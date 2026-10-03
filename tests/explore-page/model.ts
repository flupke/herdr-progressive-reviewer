// The models of the e2e agent, and the only file that knows how they are reached: one model for
// `agent.act`, and a judge for `agent.assert`, `agent.waitFor` and `agent.extract`. Each route
// fixes both, so switching to a costlier model means editing this file. The first route
// available wins (docs/development.md, "Agent steps and the model"):
//
// 1. A ChatGPT login stored by `npx e2e login openai`, through e2e's `chatgpt()`.
// 2. Anthropic, with the key in ANTHROPIC_API_KEY (run.sh reads it from ~/.secrets when the
//    environment has none), and ANTHROPIC_WORKSPACE_ID when the key needs it.
//
// An act step with a valid recording under `.e2e/cache` replays without the model; a judgment
// always calls the judge. With neither route, a call fails at once with the message below.
import { readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';
import { createAnthropic } from '@ai-sdk/anthropic';
import type { AgentOptions } from 'e2e';
import { chatgpt } from 'e2e/oauth/chatgpt';

// The smallest model the login serves (`npx e2e models openai`): it acts and judges, as the login
// offers nothing cheaper for the judge.
const CHATGPT_MODEL = 'gpt-6-luna';
const ANTHROPIC_ACT_MODEL = 'claude-sonnet-5-5';
const ANTHROPIC_JUDGE_MODEL = 'claude-haiku-4-5-20251001';

// True when e2e's credential store holds a ChatGPT login. e2e does not export its store, so this
// repeats how node_modules/e2e/dist/oauth/store.js finds and reads it (check it again when e2e
// moves to another version): E2E_OAUTH_CREDENTIALS, else $XDG_CONFIG_HOME/e2e/oauth.json, else
// ~/.config/e2e/oauth.json, and a login is an `openai` entry with string tokens and a numeric
// expiry. A missing file is no login; a file that cannot be read or parsed fails the run, as it
// would in e2e, instead of quietly switching routes. Nothing read here leaves this function.
function chatgptLoginStored(): boolean {
  let source = 'E2E_OAUTH_CREDENTIALS';
  let text = process.env.E2E_OAUTH_CREDENTIALS;
  if (!text) {
    source = join(process.env.XDG_CONFIG_HOME || join(homedir(), '.config'), 'e2e', 'oauth.json');
    try {
      text = readFileSync(source, 'utf8');
    } catch (cause) {
      if ((cause as NodeJS.ErrnoException).code === 'ENOENT') return false;
      throw new Error(`cannot read the e2e logins in ${source}`, { cause });
    }
  }
  let logins: unknown;
  try {
    logins = JSON.parse(text);
  } catch {
    throw new Error(`the e2e logins in ${source} are not valid JSON`);
  }
  if (typeof logins !== 'object' || logins === null || Array.isArray(logins)) {
    throw new Error(`the e2e logins in ${source} must be an object keyed by provider id`);
  }
  const login = (logins as { openai?: Record<string, unknown> }).openai;
  return typeof login?.access === 'string' && typeof login.refresh === 'string' && typeof login.expires === 'number';
}

const noKey: typeof fetch = async () =>
  new Response(
    JSON.stringify({
      type: 'error',
      error: {
        type: 'authentication_error',
        message:
          'no model: run `npx e2e login openai` in the dev shell, set ANTHROPIC_API_KEY, or add an ANTHROPIC_API_KEY line to ~/.secrets',
      },
    }),
    { status: 401, headers: { 'content-type': 'application/json' } },
  );

function chatgptModels(): AgentOptions {
  const model = chatgpt(CHATGPT_MODEL);
  return { model, judge: model };
}

function anthropicModels(): AgentOptions {
  const workspace = process.env.ANTHROPIC_WORKSPACE_ID;
  const anthropic = process.env.ANTHROPIC_API_KEY
    ? createAnthropic({ headers: workspace ? { 'anthropic-workspace-id': workspace } : {} })
    : createAnthropic({ apiKey: 'none', fetch: noKey });
  return { model: anthropic(ANTHROPIC_ACT_MODEL), judge: anthropic(ANTHROPIC_JUDGE_MODEL) };
}

export const models = chatgptLoginStored() ? chatgptModels() : anthropicModels();
