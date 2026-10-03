// Each test gets a fresh session on the standalone page server, and plays the agent in it.
import { test as base } from '@e2e-dev/web';

/** A session of the standalone server, which stands in for the review tool's Explore session. */
export interface Session {
  /** The token of the page's address. */
  readonly token: string;
  /** Opens the page through the address the tool gives the reviewer, with the session's token. */
  open(): Promise<void>;
  /** The agent posts its next question, the fixed question of the standalone server. */
  askQuestion(): Promise<void>;
}

async function control(baseUrl: string | undefined, path: string): Promise<Response> {
  const response = await fetch(new URL(path, baseUrl), { method: 'POST' });
  if (!response.ok) throw new Error(`POST ${path}: ${response.status} ${await response.text()}`);
  return response;
}

export const test = base.extend<{ explore: Session }>({
  explore: async ({ app }, use) => {
    const { token } = (await (await control(app.baseUrl, '/test/sessions')).json()) as { token: string };
    await use({
      token,
      open: () => app.open(`/?token=${token}`),
      askQuestion: async () => {
        await control(app.baseUrl, `/test/sessions/${token}/question`);
      },
    });
  },
});
