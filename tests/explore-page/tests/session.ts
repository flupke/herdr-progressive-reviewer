// Each test gets a fresh session on the standalone page server, and plays the agent and the
// reviewer's pane in it.
import { test as base } from '@e2e-dev/web';

/** An answer the reviewer sent from the page, as the session received it. */
export interface SentAnswer {
  question: string;
  version: number;
  choice: string | null;
  comment: string;
  /** The reviewer's first pick, by choice ID, when the question hid its recommendation. */
  first_pick?: string;
}

/** A round the reviewer started from the page, as the session received it. */
export interface SentStart {
  challenger: boolean;
}

/** A session of the standalone server, which stands in for the review tool's Explore session. */
export interface Session {
  /** The token of the page's address. */
  readonly token: string;
  /** Opens the page through the address the tool gives the reviewer, with the session's token. */
  open(): Promise<void>;
  /**
   * The agent posts its next question: `question` when given, a JSON `Question` of
   * `review_explore`, or else the fixed questions of the standalone server in turn.
   */
  askQuestion(question?: object): Promise<void>;
  /** The reviewer answers in the pane, and the agent works on its next turn. */
  answerInPane(): Promise<void>;
  /** The reviewer cancels the latest answer in the pane: its question waits again. */
  cancelAnswerInPane(): Promise<void>;
  /** The prompt of the agent's next turn could not be delivered. */
  failDelivery(): Promise<void>;
  /** The answers the reviewer sent from the page, in order. */
  answers(): Promise<SentAnswer[]>;
  /** The diagram errors the page reported, each once: what the tool saves with a question. */
  diagramErrors(): Promise<unknown[]>;
  /** The agent stops before its next turn. */
  interrupt(): Promise<void>;
  /** The agent concludes the round with the standalone server's fixed conclusion. */
  conclude(): Promise<void>;
  /** The reviewer resets the round in the pane: no round is running. */
  reset(): Promise<void>;
  /**
   * The tool sends the kickoff of the round the reviewer started, from the page or in the pane:
   * the agent works on its first turn.
   */
  sendKickoff(): Promise<void>;
  /** The round the reviewer started could not start: no round is running. */
  failStart(): Promise<void>;
  /** The rounds the reviewer started from the page, in order. */
  starts(): Promise<SentStart[]>;
}

async function control(baseUrl: string | undefined, path: string, body?: object): Promise<Response> {
  const response = await fetch(new URL(path, baseUrl), {
    method: 'POST',
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!response.ok) throw new Error(`POST ${path}: ${response.status} ${await response.text()}`);
  return response;
}

export const test = base.extend<{ explore: Session }>({
  explore: async ({ app }, use) => {
    const { token } = (await (await control(app.baseUrl, '/test/sessions')).json()) as { token: string };
    const step = async (name: string, body?: object) => {
      await control(app.baseUrl, `/test/sessions/${token}/${name}`, body);
    };
    const read = async <T>(name: string): Promise<T> => {
      const response = await fetch(new URL(`/test/sessions/${token}/${name}`, app.baseUrl));
      if (!response.ok) throw new Error(`GET ${name}: ${response.status}`);
      return (await response.json()) as T;
    };
    await use({
      token,
      open: () => app.open(`/?token=${token}`),
      askQuestion: (question?: object) => step('question', question),
      answerInPane: () => step('answer'),
      cancelAnswerInPane: () => step('cancel'),
      failDelivery: () => step('fail'),
      answers: () => read<SentAnswer[]>('answers'),
      diagramErrors: () => read<unknown[]>('diagram-errors'),
      interrupt: () => step('interrupt'),
      conclude: () => step('conclude'),
      reset: () => step('reset'),
      sendKickoff: () => step('kickoff'),
      failStart: () => step('fail-start'),
      starts: () => read<SentStart[]>('starts'),
    });
  },
});
