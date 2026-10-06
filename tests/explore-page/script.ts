// Runs a throwaway Playwright script against a fresh session of the standalone page server
// (`make explore-script SCRIPT=path.ts`, .agents/wiki/explore-page-standalone.md): an agent's
// hands on the page. It prints what the script prints, then the path of each screenshot the
// script took, all written under a new folder of the system temporary directory. It stores
// nothing in the repository, calls no model, and needs no network.
//
// The script's default export receives a `ScriptContext`:
//
//   import type { ScriptContext } from '<repository>/tests/explore-page/script.ts';
//   export default async ({ page, session, shot }: ScriptContext) => {
//     await session.open();
//     await session.askQuestion();
//     await page.getByRole('link', { name: 'Go to question 1' }).click();
//     await shot('question-1');
//     console.log(await page.getByRole('region', { name: 'Question 1' }).innerText());
//   };
//
// The script imports nothing but types: the packages resolve from this folder, not the script's.
//
// Environment:
// - SCRIPT: the script, relative to the repository's root or absolute.
// - SCRIPT_DATA: the data set of the session's agent, `short` (default) or `rich`.
// - SCRIPT_WIDTH: the window's width in CSS pixels (default 1280); its height is 900, or 844
//   below 600 pixels.
// - E2E_CHROMIUM: the Chromium the dev shell provides; Playwright's own download without it.
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import type { Browser, Page } from 'playwright';
import { launch, settle, StandaloneServer } from './standalone.ts';
import { type DataSet, openSession, type Session } from './tests/session.ts';

/** What a script works with. */
export interface ScriptContext {
  /** The browser page, opened on nothing yet: `session.open()` loads the session's round. */
  page: Page;
  /** The session, whose helpers move the round as the agent and the pane would. */
  session: Session;
  /** The server's address, such as `http://127.0.0.1:41234`. */
  baseUrl: string;
  /** The browser, for another page or context. */
  browser: Browser;
  /**
   * Opens another session on the server, whose page opens in `page`, of the data set `data`
   * or the script's.
   */
  openSession: (page: Page, data?: DataSet) => Promise<Session>;
  /** Waits until the page has drawn the round, then takes a full-page screenshot named `name`. */
  shot: (name: string) => Promise<string>;
}

/** The options of a run, from the environment. */
class Options {
  readonly script: string;
  readonly data: DataSet;
  readonly width: number;

  constructor(env: NodeJS.ProcessEnv) {
    if (!env.SCRIPT) throw new Error('name the script: make explore-script SCRIPT=path/to/script.ts');
    this.script = resolve(import.meta.dirname, '../..', env.SCRIPT);
    const data = env.SCRIPT_DATA || 'short';
    if (data !== 'short' && data !== 'rich') throw new Error(`SCRIPT_DATA must be short or rich, not ${data}`);
    this.data = data;
    this.width = Number(env.SCRIPT_WIDTH || 1280);
    if (!Number.isInteger(this.width) || this.width < 200) throw new Error(`invalid width ${env.SCRIPT_WIDTH}`);
  }
}

async function main(): Promise<void> {
  const options = new Options(process.env);
  const run = (await import(pathToFileURL(options.script).href)).default;
  if (typeof run !== 'function') throw new Error(`${options.script} has no default export to run`);
  // The folder of the screenshots, made at the first one.
  let output: string | undefined;
  const shots: string[] = [];
  const server = await StandaloneServer.start(['--data', options.data]);
  const { browser, close } = await launch();
  try {
    const context = await browser.newContext({
      viewport: { width: options.width, height: options.width < 600 ? 844 : 900 },
    });
    const page = await context.newPage();
    const sessionOf = (on: Page, data: DataSet = options.data) =>
      openSession(server.baseUrl, { open: (path) => on.goto(new URL(path, server.baseUrl).href) }, data);
    const shot = async (name: string) => {
      await settle(page);
      output ??= mkdtempSync(join(tmpdir(), 'explore-script-'));
      const file = join(output, `${String(shots.length + 1).padStart(2, '0')}-${name}.png`);
      await page.screenshot({ path: file, fullPage: true, animations: 'disabled' });
      shots.push(file);
      return file;
    };
    const scriptContext: ScriptContext = {
      page,
      session: await sessionOf(page),
      baseUrl: server.baseUrl,
      browser,
      openSession: sessionOf,
      shot,
    };
    await run(scriptContext);
  } finally {
    await close();
    server.stop();
    if (shots.length > 0) console.log(`\nScreenshots:\n${shots.map((file) => `  ${file}`).join('\n')}`);
  }
}

await main();
