// The model of the e2e agent, and the only file that knows how it is reached: Anthropic's
// claude-sonnet-5-5, with the key in ANTHROPIC_API_KEY (run.sh reads it from ~/.secrets when the
// environment has none). If the key needs a workspace ID, it is read from ANTHROPIC_WORKSPACE_ID.
//
// A step with a valid recording under `.e2e/cache` replays without the model. With no key, a
// step that needs the model fails at once with the message below; replayed steps still pass.
import { createAnthropic } from '@ai-sdk/anthropic';

const MODEL = 'claude-sonnet-5-5';

const noKey: typeof fetch = async () =>
  new Response(
    JSON.stringify({
      type: 'error',
      error: {
        type: 'authentication_error',
        message: 'no Anthropic key: set ANTHROPIC_API_KEY, or add an ANTHROPIC_API_KEY line to ~/.secrets',
      },
    }),
    { status: 401, headers: { 'content-type': 'application/json' } },
  );

const workspace = process.env.ANTHROPIC_WORKSPACE_ID;

export const model = process.env.ANTHROPIC_API_KEY
  ? createAnthropic({ headers: workspace ? { 'anthropic-workspace-id': workspace } : {} })(MODEL)
  : createAnthropic({ apiKey: 'none', fetch: noKey })(MODEL);
