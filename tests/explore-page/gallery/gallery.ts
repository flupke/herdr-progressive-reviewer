// The screenshot gallery of the Explore page (`make explore-gallery`, docs/development.md):
// starts the standalone page server with the rich data set, moves a fresh session to each
// state of states.ts, and takes a full-page screenshot of it at each width and theme with the
// dev shell's headless Chromium, then writes the contact sheet. Each screenshot gets its own
// session and its own page, loaded at its width and theme. No model, no network.
//
// Environment:
// - GALLERY_DIR: the folder to write, new or empty; by default a new folder under the system
//   temporary directory, which the run prints.
// - GALLERY_COMPARE: an earlier gallery's folder; the contact sheet then shows before and after
//   for each image that differs.
// - GALLERY_WIDTHS: the widths in CSS pixels, separated by spaces (default "1280 390").
// - GALLERY_STATES: only the states of these names, separated by spaces.
// - E2E_CHROMIUM: the Chromium the dev shell provides; Playwright's own download without it.
import { type ChildProcess, spawn } from 'node:child_process';
import { existsSync, mkdirSync, readdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { createInterface } from 'node:readline';
import { type Browser, chromium, type Page } from 'playwright';
import { startChromium, stopChromium } from '../nix-chromium.ts';
import { SERVER } from '../server.ts';
import { openSession } from '../tests/session.ts';
import { contactSheet, type Shot, THEMES, type Theme } from './contact-sheet.ts';
import { type GalleryState, STATES } from './states.ts';

/** The height of the window, which a full-page screenshot extends to the whole page. */
const height = (width: number) => (width < 600 ? 844 : 900);

/** The states, widths and folders of a run, from the environment. */
class Options {
  readonly states: GalleryState[];
  readonly widths: number[];
  readonly output: string;
  readonly compare: string | undefined;

  constructor(env: NodeJS.ProcessEnv) {
    const words = (value: string | undefined) => (value ?? '').split(/\s+/).filter(Boolean);
    const names = words(env.GALLERY_STATES);
    const unknown = names.filter((name) => !STATES.some((state) => state.name === name));
    if (unknown.length > 0) throw new Error(`unknown states: ${unknown.join(' ')}`);
    this.states = names.length > 0 ? STATES.filter((state) => names.includes(state.name)) : STATES;
    this.widths = words(env.GALLERY_WIDTHS || '1280 390').map((word) => {
      const width = Number(word);
      if (!Number.isInteger(width) || width < 200) throw new Error(`invalid width ${word}`);
      return width;
    });
    const stamp = new Date().toISOString().replace(/\..*/, '').replaceAll(':', '-');
    this.output = resolve(env.GALLERY_DIR || join(tmpdir(), 'explore-gallery', stamp));
    this.compare = env.GALLERY_COMPARE ? resolve(env.GALLERY_COMPARE) : undefined;
    if (this.compare && !existsSync(this.compare)) throw new Error(`no folder ${this.compare}`);
  }
}

/** The standalone page server, with the rich data set, on a free port. */
class Server {
  private readonly child: ChildProcess;
  readonly baseUrl: string;

  private constructor(child: ChildProcess, baseUrl: string) {
    this.child = child;
    this.baseUrl = baseUrl;
  }

  static async start(): Promise<Server> {
    const child = spawn(SERVER, ['--port', '0', '--data', 'rich'], {
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
    return new Server(child, baseUrl);
  }

  stop(): void {
    this.child.kill();
  }
}

/** The headless Chromium: the dev shell's when it names one, attached over CDP. */
async function launch(): Promise<{ browser: Browser; close: () => Promise<void> }> {
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
 * failed. A state's moves leave the page showing the round's latest view: the page was opened
 * after the round moved, or the action it sent has its reply, which follows the view.
 */
async function settle(page: Page): Promise<void> {
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
    sources === failed
  );
}

/**
 * Makes the window as tall as the page, so that a part sized to the window, such as the sticky
 * column of the reviewer's actions, shows whole instead of scrolling inside itself.
 */
async function showWhole(page: Page, width: number): Promise<void> {
  for (let tries = 0; tries < 3; tries++) {
    const whole = await page.evaluate(() => document.documentElement.scrollHeight);
    if (whole <= page.viewportSize()!.height) return;
    await page.setViewportSize({ width, height: Math.max(whole, height(width)) });
  }
}

/** Where a gallery's screenshots come from: the browser, and the server it opens pages of. */
interface Studio {
  browser: Browser;
  baseUrl: string;
}

/** Takes the screenshot of `state` at `width` in `theme`, into `output`. */
async function shoot(studio: Studio, state: GalleryState, width: number, theme: Theme, output: string): Promise<Shot> {
  const context = await studio.browser.newContext({
    viewport: { width, height: height(width) },
    colorScheme: theme,
    reducedMotion: 'reduce',
  });
  try {
    const page = await context.newPage();
    const session = await openSession(studio.baseUrl, {
      open: (path) => page.goto(new URL(path, studio.baseUrl).href),
    });
    await state.reach(session, page);
    await settle(page);
    await showWhole(page, width);
    const file = `${state.name}-${width}-${theme}.png`;
    await page.screenshot({ path: join(output, file), fullPage: true, animations: 'disabled' });
    return { state: state.name, width, theme, file };
  } finally {
    await context.close();
  }
}

/** Runs `jobs`, at most `limit` at a time, and returns their results in order. */
async function pooled<T>(jobs: (() => Promise<T>)[], limit: number): Promise<T[]> {
  const results: T[] = new Array(jobs.length);
  let next = 0;
  const worker = async () => {
    while (next < jobs.length) {
      const index = next++;
      results[index] = await jobs[index]!();
    }
  };
  await Promise.all(Array.from({ length: Math.min(limit, jobs.length) }, worker));
  return results;
}

async function main(): Promise<void> {
  const options = new Options(process.env);
  if (existsSync(options.output) && readdirSync(options.output).length > 0) {
    throw new Error(`${options.output} is not empty: choose a new or empty folder`);
  }
  mkdirSync(options.output, { recursive: true });
  const server = await Server.start();
  const { browser, close } = await launch();
  try {
    const studio: Studio = { browser, baseUrl: server.baseUrl };
    const jobs = options.states.flatMap((state) =>
      options.widths.flatMap((width) =>
        THEMES.map((theme) => async () => {
          try {
            return await shoot(studio, state, width, theme, options.output);
          } catch (error) {
            const message = error instanceof Error ? error.message : String(error);
            throw new Error(`state ${state.name}, ${width} px, ${theme}: ${message}`);
          }
        }),
      ),
    );
    const shots = await pooled(jobs, 4);
    const sheet = contactSheet(options.states, shots, options.output, options.compare);
    writeFileSync(join(options.output, 'index.html'), sheet.html);
    console.log(`${shots.length} screenshots of ${options.states.length} states${sheet.summary}`);
    console.log(`Explore gallery: ${join(options.output, 'index.html')}`);
  } finally {
    await close();
    server.stop();
  }
}

await main();
