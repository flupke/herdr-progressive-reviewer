// The screenshot gallery of the Explore page (`make explore-gallery`, .agents/wiki/explore-page-standalone.md):
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
import { existsSync, mkdirSync, readdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import type { Browser, Page } from 'playwright';
import { launch, settle, StandaloneServer } from '../standalone.ts';
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

/**
 * The time of the server's fixed clock (`FIXED_NOW_MS` in
 * crates/review-explore-page-server/src/sessions.rs), which stamps every start and turn: the
 * pages stand 42 seconds after it, so that a time since reads "0:42" in every run.
 */
const SERVER_CLOCK_MS = 1_791_000_000_000;
const PAGE_CLOCK_MS = SERVER_CLOCK_MS + 42_000;

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
    // Clock times read the same on every machine.
    timezoneId: 'UTC',
  });
  try {
    // The page's clock stands still at a fixed time; its timers still run.
    await context.clock.setFixedTime(PAGE_CLOCK_MS);
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
  const server = await StandaloneServer.start(['--data', 'rich', '--fixed-clock']);
  const { browser, close } = await launch();
  try {
    const studio: Studio = { browser, baseUrl: server.baseUrl };
    const jobs = options.states.flatMap((state) =>
      [...new Set([...options.widths, ...(state.extraWidths ?? [])])].flatMap((width) =>
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
