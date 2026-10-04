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

/** What the reviewer answered of a conclusion's quiz, as the review tool saves it. */
export interface QuizAnswers {
  /** Each item picked, from 0, with the option picked, from 0; left out while empty. */
  picks?: { item: number; answer: number; correct: boolean }[];
  /** Whether the reviewer skipped the rest of the quiz; left out when not. */
  skipped?: boolean;
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
  /**
   * The reviewer answers with the recommended choice (the first one when none is) after a first
   * pick of another choice, as on a blind question on the page; the agent works on its next turn.
   */
  answerAfterFirstPick(): Promise<void>;
  /** The reviewer cancels the latest answer in the pane: its question waits again. */
  cancelAnswerInPane(): Promise<void>;
  /**
   * The prompt the session sends could not be delivered: the conclusion's implementation
   * request, while the session sends one, or else the prompt of the agent's next turn.
   */
  failDelivery(): Promise<void>;
  /**
   * The agent did not start on the prompt the session sends: the conclusion's implementation
   * request, while the session sends one, or else the prompt of its next turn.
   */
  agentDoesNotStart(): Promise<void>;
  /**
   * The reviewer reopens the review before the prompt the session sends went out: the
   * conclusion's implementation request, while the session sends one, is saved but not sent;
   * or else the agent's next turn stopped.
   */
  reopenBeforeSending(): Promise<void>;
  /**
   * The reviewer reopens the review while the prompt the session sends was being delivered:
   * whether the agent received it is unknown.
   */
  reopenWhileSending(): Promise<void>;
  /** Another reviewer saved a newer round of the review: this one offers only Reset. */
  becomeEarlierRound(): Promise<void>;
  /** The review tool cannot save the reviewer's rounds any more. */
  failStorage(): Promise<void>;
  /** The answers the reviewer sent from the page, in order. */
  answers(): Promise<SentAnswer[]>;
  /** The diagram errors the page reported, each once: what the tool saves with a question. */
  diagramErrors(): Promise<unknown[]>;
  /** The reviewer implements the conclusion in the pane, and the agent receives the request. */
  implementInPane(): Promise<void>;
  /** The agent receives the implementation request the session sends. */
  deliverImplementation(): Promise<void>;
  /** The lists to be implemented that the reviewer sent from the page, in order. */
  implementations(): Promise<string[]>;
  /** The agent stops before its next turn. */
  interrupt(): Promise<void>;
  /** The agent concludes the round with the standalone server's fixed conclusion. */
  conclude(): Promise<void>;
  /** The agent concludes the round with the fixed conclusion and its quiz of two items. */
  concludeWithQuiz(): Promise<void>;
  /** What the reviewer answered of the conclusion's quiz from the page. */
  quiz(): Promise<QuizAnswers>;
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
  /** The reviewer marks every changed line as reviewed: nothing is left to review. */
  reviewEverything(): Promise<void>;
  /** The reviewer unmarks a line: a round can start again. */
  unreviewLine(): Promise<void>;
  /**
   * The other actions the reviewer took on the page to recover or close the round, by name, in
   * order: `stop`, `retry`, `cancel-answer`, `reset`, `reply`, `cancel-implementation`,
   * `resend-implementation`.
   */
  actions(): Promise<string[]>;
  /**
   * The page stops following the round, as a page whose socket does not get the tool's
   * messages: it keeps showing the round as it was until the reviewer acts on it. A test of a
   * refusal of an action on a stale page holds the page first, then moves the round.
   */
  holdPage(): Promise<void>;
  /**
   * The reviewer restarts: the page's socket closes, and the page cannot open another one until
   * `reviewerBack()`. The round stays where it is, and the test may move it meanwhile.
   */
  restartReviewer(): Promise<void>;
  /** The restarted reviewer is back: the page opens its socket again. */
  reviewerBack(): Promise<void>;
}

async function control(baseUrl: string | undefined, path: string, body?: object): Promise<Response> {
  const response = await fetch(new URL(path, baseUrl), {
    method: 'POST',
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!response.ok) throw new Error(`POST ${path}: ${response.status} ${await response.text()}`);
  return response;
}

/** The browser page a session's page opens in. */
export interface SessionPage {
  /** Loads `path` of the server. */
  open(path: string): Promise<unknown>;
}

/**
 * Opens a session on the server at `baseUrl`, whose page opens in `page`. The `explore`
 * fixture and the screenshot gallery (`gallery/`) both play rounds through it.
 */
export async function openSession(baseUrl: string | undefined, page: SessionPage): Promise<Session> {
  const { token } = (await (await control(baseUrl, '/test/sessions')).json()) as { token: string };
  const step = async (name: string, body?: object) => {
    await control(baseUrl, `/test/sessions/${token}/${name}`, body);
  };
  const read = async <T>(name: string): Promise<T> => {
    const response = await fetch(new URL(`/test/sessions/${token}/${name}`, baseUrl));
    if (!response.ok) throw new Error(`GET ${name}: ${response.status}`);
    return (await response.json()) as T;
  };
  return {
    token,
    open: async () => {
      await page.open(`/?token=${token}`);
    },
    askQuestion: (question?: object) => step('question', question),
    answerInPane: () => step('answer'),
    answerAfterFirstPick: () => step('answer-after-first-pick'),
    cancelAnswerInPane: () => step('cancel'),
    failDelivery: () => step('fail'),
    agentDoesNotStart: () => step('not-started'),
    reopenBeforeSending: () => step('reopen-unsent'),
    reopenWhileSending: () => step('reopen-sending'),
    becomeEarlierRound: () => step('earlier'),
    failStorage: () => step('fail-storage'),
    answers: () => read<SentAnswer[]>('answers'),
    diagramErrors: () => read<unknown[]>('diagram-errors'),
    implementInPane: () => step('implement'),
    deliverImplementation: () => step('deliver'),
    implementations: () => read<string[]>('implementations'),
    interrupt: () => step('interrupt'),
    conclude: () => step('conclude'),
    concludeWithQuiz: () => step('conclude-with-quiz'),
    quiz: () => read<QuizAnswers>('quiz'),
    reset: () => step('reset'),
    sendKickoff: () => step('kickoff'),
    failStart: () => step('fail-start'),
    starts: () => read<SentStart[]>('starts'),
    reviewEverything: () => step('review-everything'),
    unreviewLine: () => step('unreview-line'),
    actions: () => read<string[]>('actions'),
    holdPage: () => step('hold'),
    restartReviewer: () => step('restart'),
    reviewerBack: () => step('back'),
  };
}

export const test = base.extend<{ explore: Session }>({
  explore: async ({ app }, use) => {
    const page: SessionPage = { open: (path) => app.open(path) };
    await use(await openSession(app.baseUrl, page));
  },
});
