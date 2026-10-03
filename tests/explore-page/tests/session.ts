// Each test gets a fresh session on the standalone page server, and plays the agent and the
// reviewer's pane in it.
import { test as base } from '@e2e-dev/web';

/** An answer the reviewer sent from the page, as the session received it. */
export interface SentAnswer {
  question: string;
  version: number;
  choice: string | null;
  comment: string;
}

/** A session of the standalone server, which stands in for the review tool's Explore session. */
export interface Session {
  /** The token of the page's address. */
  readonly token: string;
  /** Opens the page through the address the tool gives the reviewer, with the session's token. */
  open(): Promise<void>;
  /** The agent posts its next question: the fixed questions of the standalone server in turn. */
  askQuestion(): Promise<void>;
  /** The reviewer answers in the pane, and the agent works on its next turn. */
  answerInPane(): Promise<void>;
  /** The reviewer cancels the latest answer in the pane: its question waits again. */
  cancelAnswerInPane(): Promise<void>;
  /** The prompt of the agent's next turn could not be delivered. */
  failDelivery(): Promise<void>;
  /** The answers the reviewer sent from the page, in order. */
  answers(): Promise<SentAnswer[]>;
  /** The agent stops before its next turn. */
  interrupt(): Promise<void>;
  /** The agent concludes the round with the standalone server's fixed conclusion. */
  conclude(): Promise<void>;
  /** The reviewer resets the round in the pane: no round is running. */
  reset(): Promise<void>;
}

async function control(baseUrl: string | undefined, path: string): Promise<Response> {
  const response = await fetch(new URL(path, baseUrl), { method: 'POST' });
  if (!response.ok) throw new Error(`POST ${path}: ${response.status} ${await response.text()}`);
  return response;
}

export const test = base.extend<{ explore: Session }>({
  explore: async ({ app }, use) => {
    const { token } = (await (await control(app.baseUrl, '/test/sessions')).json()) as { token: string };
    const step = async (name: string) => {
      await control(app.baseUrl, `/test/sessions/${token}/${name}`);
    };
    await use({
      token,
      open: () => app.open(`/?token=${token}`),
      askQuestion: () => step('question'),
      answerInPane: () => step('answer'),
      cancelAnswerInPane: () => step('cancel'),
      failDelivery: () => step('fail'),
      answers: async () => {
        const response = await fetch(new URL(`/test/sessions/${token}/answers`, app.baseUrl));
        if (!response.ok) throw new Error(`GET answers: ${response.status}`);
        return (await response.json()) as SentAnswer[];
      },
      interrupt: () => step('interrupt'),
      conclude: () => step('conclude'),
      reset: () => step('reset'),
    });
  },
});
