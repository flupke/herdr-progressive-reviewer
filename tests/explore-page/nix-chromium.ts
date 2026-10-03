// Lease the dev shell's Chromium (E2E_CHROMIUM, from nixpkgs) as a "hosted"
// browser. Playwright's downloaded build is linked against an FHS system and does not run on
// NixOS, and the web engine has no option for a browser executable, so the engine attaches to
// this one over CDP instead. One headless Chromium per worker slot, killed on release.
import { spawn } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import type { BrowserProvider } from '@e2e-dev/web';

interface LocalLease {
  id: string;
  cdpEndpoint: string;
  pid: number;
  profile: string;
}

// What Playwright passes when it launches Chromium itself (playwright-core 1.63,
// chromiumSwitches), minus what only matters for a headed or a persistent browser. Without
// them Chromium talks to Google services in the background.
const SWITCHES = [
  '--headless',
  '--remote-debugging-port=0',
  '--disable-field-trial-config',
  '--disable-background-networking',
  '--disable-background-timer-throttling',
  '--disable-backgrounding-occluded-windows',
  '--disable-back-forward-cache',
  '--disable-breakpad',
  '--disable-client-side-phishing-detection',
  '--disable-component-extensions-with-background-pages',
  '--disable-component-update',
  '--no-default-browser-check',
  '--disable-default-apps',
  '--disable-dev-shm-usage',
  '--disable-extensions',
  '--disable-features=AvoidUnnecessaryBeforeUnloadCheckSync,DestroyProfileOnBrowserClose,' +
    'DialMediaRouteProvider,GlobalMediaControls,HttpsUpgrades,LensOverlay,MediaRouter,' +
    'PaintHolding,ThirdPartyStoragePartitioning,Translate,AutoDeElevate,OptimizationHints,' +
    // Not in Playwright's list: stops the secure DNS probe to a public resolver.
    'DnsOverHttps',
  '--enable-automation',
  '--disable-domain-reliability',
  '--allow-pre-commit-input',
  '--disable-hang-monitor',
  '--disable-ipc-flooding-protection',
  '--disable-popup-blocking',
  '--disable-prompt-on-repost',
  '--disable-renderer-backgrounding',
  '--force-color-profile=srgb',
  '--metrics-recording-only',
  '--no-first-run',
  '--password-store=basic',
  '--use-mock-keychain',
  '--no-service-autorun',
  '--disable-search-engine-choice-screen',
  '--disable-sync',
  '--no-pings',
  '--mute-audio',
  '--hide-scrollbars',
];

function alive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

// Stops the browser's process group, then removes its profile.
async function stop(pid: number | undefined, profile: string): Promise<void> {
  if (pid !== undefined) {
    try {
      process.kill(-pid, 'SIGTERM');
    } catch {
      // Already gone.
    }
    for (let waited = 0; alive(pid) && waited < 5_000; waited += 50) {
      await new Promise((resolve) => setTimeout(resolve, 50));
    }
    try {
      if (alive(pid)) process.kill(-pid, 'SIGKILL');
    } catch {
      // Gone in between.
    }
  }
  rmSync(profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
}

export function nixChromium(executable: string): BrowserProvider {
  return {
    name: 'nix-chromium',
    async acquire(request) {
      const profile = mkdtempSync(join(tmpdir(), 'e2e-chromium-'));
      const child = spawn(executable, [...SWITCHES, `--user-data-dir=${profile}`, 'about:blank'], {
        stdio: ['ignore', 'ignore', 'pipe'],
        detached: true,
      });
      let timer: NodeJS.Timeout | undefined;
      let onAbort: (() => void) | undefined;
      try {
        const cdpEndpoint = await new Promise<string>((resolve, reject) => {
          let stderr = '';
          timer = setTimeout(() => reject(new Error(`chromium did not start:\n${stderr}`)), 15_000);
          child.stderr!.on('data', (chunk: Buffer) => {
            stderr += chunk.toString();
            const match = /DevTools listening on (ws:\/\/\S+)/.exec(stderr);
            if (match) resolve(match[1]!);
          });
          child.on('error', reject);
          child.on('exit', (code) => reject(new Error(`chromium exited with ${code}:\n${stderr}`)));
          onAbort = () => reject(new Error('cancelled'));
          if (request.signal.aborted) onAbort();
          request.signal.addEventListener('abort', onAbort);
        });
        child.stderr!.resume();
        child.unref();
        const lease: LocalLease = { id: `chromium-${child.pid}`, cdpEndpoint, pid: child.pid!, profile };
        return lease;
      } catch (error) {
        await stop(child.pid, profile);
        throw error;
      } finally {
        clearTimeout(timer);
        if (onAbort) request.signal.removeEventListener('abort', onAbort);
      }
    },
    async release(lease) {
      const { pid, profile } = lease as unknown as LocalLease;
      await stop(pid, profile);
    },
  };
}
