// Runs a throwaway Playwright script against a fresh session of the standalone page server
// (`make explore-script SCRIPT=path.ts`, .agents/wiki/explore-page-standalone.md): an agent's
// hands on the page. It runs the script once for each width and theme, each time in a new page
// on a new session, and prints what the script prints, then the folder of the screenshots and
// their names. It stores nothing in the repository, calls no model, and needs no network.
//
// The script's default export receives a `ScriptContext`:
//
//   import type { ScriptContext } from '<repository>/tests/explore-page/script.ts';
//   export default async ({ page, session, shot, showQuestion }: ScriptContext) => {
//     await session.open();
//     await session.askQuestion();
//     // The first time a tab shows a round, it shows the design screen, not the question.
//     await showQuestion(1);
//     await shot('question-1');
//     const question = page.getByRole('region', { name: 'Question 1' });
//     await shot('question-only', { locator: question });
//     console.log(await question.innerText());
//   };
//
// `session` moves the round as the agent and the pane would: its helpers are documented on the
// `Session` interface of tests/explore-page/tests/session.ts. The script imports nothing but
// types: the packages resolve from this folder, not the script's. It may live in any folder,
// with or without a package.json: it runs as an ES module.
//
// Each screenshot is named `<n>-<name>-<width>-<theme>.png`, numbered in the order the script
// takes them, so that one width and theme compare with another file by file. Playwright waits at
// most 10 seconds for an action. When the script fails, the runner shoots
// `failure-<width>-<theme>.png`, prints the error, the page's address and the visible text of
// its masthead and main, and stops with a failure.
//
// Environment:
// - SCRIPT: the script, relative to the repository's root or absolute. Without it,
//   `make explore-script` prints this header.
// - SCRIPT_DATA: the data set of the session's agent, `short` (default) or `rich`.
// - SCRIPT_WIDTH: the window's widths in CSS pixels, separated by commas or spaces (default
//   1280); its height is 900, or 844 below 600 pixels.
// - SCRIPT_THEME: the themes, `light` (default), `dark`, or both: `light,dark`.
// - SCRIPT_OUT: the folder of the screenshots, an absolute path, made when missing. The runner
//   first deletes the earlier screenshots there by their names' pattern, `<n>-*-<width>-<theme>.png`
//   and `failure-<width>-<theme>.png`, and leaves any other file. By default a new folder of the
//   system temporary directory.
// - E2E_CHROMIUM: the Chromium the dev shell provides; Playwright's own download without it.
import { mkdirSync, mkdtempSync, readdirSync, rmSync } from 'node:fs';
import { registerHooks } from 'node:module';
import { tmpdir } from 'node:os';
import { dirname, isAbsolute, join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import type { Browser, Locator, Page } from 'playwright';
import { launch, settle, StandaloneServer, THEMES, type Theme, windowHeight } from './standalone.ts';
import { type DataSet, openSession, type Session } from './tests/session.ts';

/** What a script works with. */
export interface ScriptContext {
  /** The browser page, opened on nothing yet: `session.open()` loads the session's round. */
  page: Page;
  /** The session, whose helpers move the round as the agent and the pane would. */
  session: Session;
  /** The window's width in CSS pixels in this run of the script. */
  width: number;
  /** The page's theme in this run of the script. */
  theme: Theme;
  /** The server's address, such as `http://127.0.0.1:41234`. */
  baseUrl: string;
  /** The browser, for another page or context. */
  browser: Browser;
  /**
   * Opens another session on the server, whose page opens in `page`, of the data set `data`
   * or the script's.
   */
  openSession: (page: Page, data?: DataSet) => Promise<Session>;
  /**
   * Shows question `number` once the round asks it. From the design screen, which a tab shows
   * instead of the question the first time it shows a round, it follows "Go to question
   * `number`" as the reviewer would.
   */
  showQuestion: (number: number) => Promise<void>;
  /** Waits until the page has drawn the round, then takes a screenshot named `name`. */
  shot: (name: string, options?: ShotOptions) => Promise<string>;
}

/** What a screenshot shows. */
export interface ShotOptions {
  /** Only this element, which Playwright scrolls into view. */
  locator?: Locator;
  /** The whole page when true (default), or only the window; ignored with `locator`. */
  fullPage?: boolean;
}

/** One run of the script: a width and a theme. */
interface Look {
  width: number;
  theme: Theme;
}

/** The options of a run, from the environment. */
class Options {
  readonly script: string;
  readonly data: DataSet;
  readonly looks: Look[];
  /** SCRIPT_OUT, when given. */
  readonly output: string | undefined;

  constructor(env: NodeJS.ProcessEnv) {
    const root = resolve(import.meta.dirname, '../..');
    if (!env.SCRIPT) throw new Error('name the script: make explore-script SCRIPT=path/to/script.ts');
    this.script = resolve(root, env.SCRIPT);
    const data = env.SCRIPT_DATA || 'short';
    if (data !== 'short' && data !== 'rich') throw new Error(`SCRIPT_DATA must be short or rich, not ${data}`);
    this.data = data;
    const words = (value: string) => value.split(/[\s,]+/).filter(Boolean);
    const widths = words(env.SCRIPT_WIDTH || '1280').map((word) => {
      const width = Number(word);
      if (!Number.isInteger(width) || width < 200) throw new Error(`invalid width ${word}`);
      return width;
    });
    const themes = words(env.SCRIPT_THEME || 'light').map((word) => {
      const theme = THEMES.find((known) => known === word);
      if (!theme) throw new Error(`SCRIPT_THEME must name light or dark, not ${word}`);
      return theme;
    });
    this.looks = widths.flatMap((width) => themes.map((theme) => ({ width, theme })));
    if (env.SCRIPT_OUT && !isAbsolute(env.SCRIPT_OUT)) {
      throw new Error(`SCRIPT_OUT must be an absolute path, not ${env.SCRIPT_OUT}`);
    }
    this.output = env.SCRIPT_OUT || undefined;
  }
}

/** The names of the files the runner writes in the folder of the screenshots. */
const SHOT_FILE = /^(\d{2,}-.+|failure)-\d+-(light|dark)\.png$/;

/** The folder of the screenshots: SCRIPT_OUT, or a new temporary folder made at the first one. */
class ShotFolder {
  private path: string | undefined;
  /** The names of the screenshots taken, in order. */
  private readonly names: string[] = [];

  constructor(fixed: string | undefined) {
    if (fixed === undefined) return;
    mkdirSync(fixed, { recursive: true });
    for (const entry of readdirSync(fixed, { withFileTypes: true })) {
      if (entry.isFile() && SHOT_FILE.test(entry.name)) rmSync(join(fixed, entry.name));
    }
    this.path = fixed;
  }

  /** The path of a new screenshot named `name`. */
  file(name: string): string {
    this.path ??= mkdtempSync(join(tmpdir(), 'explore-script-'));
    this.names.push(name);
    return join(this.path, name);
  }

  /** Prints the folder and the screenshots' names, if any. */
  report(): void {
    if (this.names.length === 0) return;
    console.log(`\nScreenshots in ${this.path}:\n${this.names.map((name) => `  ${name}`).join('\n')}`);
  }
}

/** What each run of the script shares: the server, the browser and the screenshots' folder. */
interface Stage {
  server: StandaloneServer;
  browser: Browser;
  folder: ShotFolder;
  data: DataSet;
}

type Script = (context: ScriptContext) => Promise<void>;

/** Runs `script` in a new page at `look`, and returns whether it passed. */
async function runAt(script: Script, look: Look, stage: Stage): Promise<boolean> {
  const { width, theme } = look;
  const context = await stage.browser.newContext({
    viewport: { width, height: windowHeight(width) },
    colorScheme: theme,
  });
  context.setDefaultTimeout(10_000);
  try {
    const page = await context.newPage();
    const baseUrl = stage.server.baseUrl;
    const sessionOf = (on: Page, data: DataSet = stage.data) =>
      openSession(baseUrl, { open: (path) => on.goto(new URL(path, baseUrl).href) }, data);
    let count = 0;
    const shot = async (name: string, options: ShotOptions = {}) => {
      await settle(page);
      count += 1;
      const file = stage.folder.file(`${String(count).padStart(2, '0')}-${name}-${width}-${theme}.png`);
      if (options.locator) await options.locator.screenshot({ path: file, animations: 'disabled' });
      else await page.screenshot({ path: file, fullPage: options.fullPage ?? true, animations: 'disabled' });
      return file;
    };
    try {
      await script({
        page,
        session: await sessionOf(page),
        width,
        theme,
        baseUrl,
        browser: stage.browser,
        openSession: sessionOf,
        showQuestion: (number) => showQuestion(page, number),
        shot,
      });
      return true;
    } catch (error) {
      await reportFailure(page, error, stage.folder.file(`failure-${width}-${theme}.png`));
      return false;
    }
  } finally {
    await context.close();
  }
}

/** `ScriptContext.showQuestion` in `page`. */
async function showQuestion(page: Page, number: number): Promise<void> {
  const question = page.getByRole('region', { name: `Question ${number}`, exact: true });
  const way = page.getByRole('link', { name: `Go to question ${number}`, exact: true });
  await question.or(way).first().waitFor();
  if (await way.isVisible()) await way.click();
  await question.waitFor();
  await settle(page);
}

/** Prints `error` and what the page shows after it, and shoots the page into `file`. */
async function reportFailure(page: Page, error: unknown, file: string): Promise<void> {
  console.error(`\nThe script failed: ${error instanceof Error ? (error.stack ?? error.message) : String(error)}`);
  try {
    await page.screenshot({ path: file, fullPage: true, animations: 'disabled', timeout: 5_000 });
  } catch (shotError) {
    console.error(`No screenshot of the failure: ${shotError instanceof Error ? shotError.message : shotError}`);
  }
  console.error(`\nAddress: ${page.url()}`);
  for (const [name, selector] of [
    ['Masthead', 'header.masthead'],
    ['Main', 'main'],
  ] as const) {
    const text = await page
      .locator(selector)
      .first()
      .innerText({ timeout: 1_000 })
      .catch(() => null);
    const lines = text === null ? '(not on the page)' : text.trim() || '(nothing visible)';
    console.error(`\n${name}:\n${lines.replace(/^/gm, '  ')}`);
  }
}

/**
 * Loads the script, and any TypeScript file of its folder that it imports, as ES modules with
 * their types stripped, whatever package.json the folder has or lacks: Node neither guesses
 * their module type nor warns about it.
 */
async function load(path: string): Promise<Script> {
  const folder = pathToFileURL(dirname(path)).href + '/';
  registerHooks({
    load: (url, context, nextLoad) => {
      const ours = url.startsWith(folder) && url.endsWith('.ts');
      return nextLoad(url, ours ? { ...context, format: 'module-typescript' } : context);
    },
  });
  const run = (await import(pathToFileURL(path).href)).default;
  if (typeof run !== 'function') throw new Error(`${path} has no default export to run`);
  return run;
}

async function main(): Promise<void> {
  const options = new Options(process.env);
  const script = await load(options.script);
  const folder = new ShotFolder(options.output);
  const server = await StandaloneServer.start(['--data', options.data]);
  const { browser, close } = await launch();
  try {
    const stage: Stage = { server, browser, folder, data: options.data };
    for (const look of options.looks) {
      if (options.looks.length > 1) console.log(`\n== ${look.width} px, ${look.theme} ==`);
      if (!(await runAt(script, look, stage))) {
        process.exitCode = 1;
        break;
      }
    }
  } finally {
    await close();
    server.stop();
    folder.report();
  }
}

await main();
