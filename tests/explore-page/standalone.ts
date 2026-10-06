// The standalone page server and a headless Chromium, started outside the e2e runner: for the
// screenshot gallery (gallery/gallery.ts) and throwaway scripts (script.ts).
import { type ChildProcess, spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { type Browser, chromium, type Page } from 'playwright';
import { startChromium, stopChromium } from './nix-chromium.ts';
import { SERVER } from './server.ts';

/** The standalone page server, on a free port. */
export class StandaloneServer {
  private readonly child: ChildProcess;
  readonly baseUrl: string;

  private constructor(child: ChildProcess, baseUrl: string) {
    this.child = child;
    this.baseUrl = baseUrl;
  }

  /** Starts the server with `args` besides its port (`--data rich`, `--fixed-clock`). */
  static async start(args: string[]): Promise<StandaloneServer> {
    const child = spawn(SERVER, ['--port', '0', ...args], {
      stdio: ['ignore', 'pipe', 'inherit'],
    });
    const lines = createInterface({ input: child.stdout! });
    const baseUrl = await new Promise<string>((resolve, reject) => {
      child.on('error', reject);
      child.on('exit', (code) => reject(new Error(`explore-page-server exited with ${code}`)));
      lines.on('line', (line) => {
        const match = /^Explore page: (http:\/\/[^/]+)\//.exec(line);
        if (match) resolve(match[1]!);
        else if (line.startsWith('csp violation') || line.startsWith('template error')) console.error(line);
      });
    });
    return new StandaloneServer(child, baseUrl);
  }

  stop(): void {
    this.child.kill();
  }
}

/** The headless Chromium: the dev shell's when it names one, attached over CDP. */
export async function launch(): Promise<{ browser: Browser; close: () => Promise<void> }> {
  const executable = process.env.E2E_CHROMIUM;
  if (!executable) {
    const browser = await chromium.launch();
    return { browser, close: () => browser.close() };
  }
  const lease = await startChromium(executable);
  const browser = await chromium.connectOverCDP(lease.cdpEndpoint);
  return {
    browser,
    close: async () => {
      await browser.close();
      await stopChromium(lease);
    },
  };
}

/**
 * Waits until the client has drawn the round, with its fonts loaded and every diagram drawn or
 * failed. Moves of the round leave the page showing the round's latest view: the page was opened
 * after the round moved, or the action it sent has its reply, which follows the view.
 */
export async function settle(page: Page): Promise<void> {
  await page.waitForFunction(drawn);
}

/** In the page: whether the client drew a view, its fonts and diagrams included. */
function drawn(): boolean {
  const sources = document.querySelectorAll('.markdown pre > code.language-mermaid').length;
  const failed = document.querySelectorAll('figure.diagram.failed').length;
  return (
    document.querySelector('main[data-seq]') !== null &&
    !document.querySelector('main[aria-busy="true"]') &&
    document.fonts.status === 'loaded' &&
    sources === failed &&
    document.body.dataset.diagrams !== 'drawing'
  );
}
