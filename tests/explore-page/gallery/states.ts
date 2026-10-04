// Every state of the Explore page that the screenshot gallery shows, in the order of the
// contact sheet. A new state of the page is one more entry: a name, a line that says what it
// shows, and how a fresh session reaches it through the fixture of the e2e tests (Session) and
// exact actions on the page. The server posts the rich data set (`--data rich`).
import type { Page } from 'playwright';
import type { Session } from '../tests/session.ts';

export interface GalleryState {
  /**
   * Names the state's screenshots, `<name>-<width>-<theme>.png`. Keep a name once it exists:
   * two runs are compared file by file.
   */
  name: string;
  /** What the page shows in this state, for the contact sheet. */
  about: string;
  /**
   * Moves a fresh session, whose agent works on its first question, to the state, and leaves
   * `page` showing it.
   */
  reach(session: Session, page: Page): Promise<void>;
}

/**
 * Clicks `name`, a button or a link, and waits until the page has the reply to the action it
 * sent, if any: the round it changed is drawn by then.
 */
async function submit(page: Page, name: string): Promise<void> {
  await page.getByRole('button', { name, exact: true }).or(page.getByRole('link', { name, exact: true })).click();
  await page.waitForFunction(() => !document.querySelector('main[aria-busy="true"]'));
  // The page changes in place, under the pointer and where the click scrolled it: move the
  // pointer away and scroll back to the top, so that no control shows as hovered and no part
  // as scrolled, as on a page loaded again.
  await page.mouse.move(0, 0);
  await page.evaluate(() => {
    for (const panel of document.querySelectorAll('.panel')) panel.scrollTop = 0;
    window.scrollTo(0, 0);
  });
}

/** Selects the choice `name` of the question or the quiz item on the page. */
async function choose(page: Page, name: string): Promise<void> {
  await page.getByRole('radio', { name }).check();
}

/** The start screen of a session whose round the reviewer reset. */
async function startScreen(session: Session): Promise<void> {
  await session.reset();
  await session.open();
}

/** The page, opened again, shows the round once `move` changed it. */
async function after(session: Session, move: () => Promise<void>): Promise<void> {
  await move();
  await session.open();
}

/**
 * The page shows question `count`, each earlier question answered in the pane. The round opens
 * on its design: on question 1 the reviewer goes on from it to the question.
 */
async function question(session: Session, page: Page, count: number): Promise<void> {
  await ask(session, count);
  if (count === 1) await submit(page, 'Go to question 1');
}

/** The page shows the design screen, on question `count`, each earlier question answered in the
 * pane: the design opens the round, and later the reviewer opens it again at its address. */
async function design(session: Session, count: number): Promise<void> {
  await ask(session, count);
  if (count > 1) await session.openDesign();
}

/** The agent asks question `count`, each earlier one answered in the pane, then the page opens. */
async function ask(session: Session, count: number): Promise<void> {
  for (let asked = 1; asked <= count; asked++) {
    if (asked > 1) await session.answerInPane();
    await session.askQuestion();
  }
  await session.open();
}

/**
 * Opens every folded part of what the page gives to read: Door, Blast radius, the other
 * citations, the lines an answer marks. A fold that holds an action opens from a button of a
 * button tier (`.button`), and stays closed.
 */
async function unfold(page: Page): Promise<void> {
  const folded = page.locator(
    'main details:not([open]) > summary:visible, main button[aria-expanded="false"]:not(.button):visible',
  );
  while ((await folded.count()) > 0) await folded.first().click();
}

/**
 * The page shows the conclusion, with its quiz when `quiz`, after the three questions: the
 * second, a blind one, answered with the recommendation after a first pick of another choice.
 */
async function conclusion(session: Session, quiz: boolean): Promise<void> {
  for (let asked = 1; asked <= 3; asked++) {
    await session.askQuestion();
    await (asked === 2 ? session.answerAfterFirstPick() : session.answerInPane());
  }
  await after(session, () => (quiz ? session.concludeWithQuiz() : session.conclude()));
}

/** The page sent the conclusion's implementation request, and the session is sending it. */
async function implementing(session: Session, page: Page): Promise<void> {
  await conclusion(session, false);
  await submit(page, IMPLEMENT);
}

/** The page shows the first item of the quiz answered right, then the second answered wrong. */
async function quizRightThenWrong(session: Session, page: Page): Promise<void> {
  await conclusion(session, true);
  await choose(page, QUIZ_RIGHT);
  await submit(page, 'Check');
  await submit(page, 'Next question');
  await choose(page, QUIZ_WRONG);
  await submit(page, 'Check');
}

/**
 * Moves the round with `move` behind the page's back, then takes the page's action `action`,
 * which the round no longer offers: the page shows the refusal's notice, over the round as it
 * is now.
 */
async function refused(session: Session, page: Page, move: () => Promise<void>, action: string): Promise<void> {
  await session.holdPage();
  await move();
  await submit(page, action);
}

// Choices and quiz answers of the rich data set (crates/review-explore-page-server/src/rich).
const IDLE_OR_FULL = 'Send the queue after two seconds';
const MERGE = 'Keep sending each reply at once';
const DROP = 'Drop the waiting notifications';
const CLOSE = 'Send them in one last notification';
const QUIZ_RIGHT = 'Once, about two seconds after the fifth reply';
const QUIZ_WRONG = 'The reply itself, which was only in the queue';

// The conclusion's Implement, which counts the ten items of its list.
const IMPLEMENT = 'Implement 10 items';

const COMMENT =
  'Two seconds feels short when I read the cited code between replies; I would rather see the count of waiting replies first.';

export const STATES: GalleryState[] = [
  {
    name: 'start',
    about: 'No round is running: the review, Start and Start with Challenger.',
    reach: (session) => startScreen(session),
  },
  {
    name: 'start-nothing-to-review',
    about: 'No round can start: every changed line is reviewed, and the buttons say why.',
    async reach(session) {
      await session.reviewEverything();
      await startScreen(session);
    },
  },
  {
    name: 'starting',
    about: 'The reviewer pressed Start: the round is being prepared, with Stop waiting.',
    async reach(session, page) {
      await startScreen(session);
      await submit(page, 'Start');
    },
  },
  {
    name: 'start-failed',
    about: 'The round the reviewer started could not start.',
    async reach(session, page) {
      await startScreen(session);
      await submit(page, 'Start');
      await after(session, () => session.failStart());
    },
  },
  {
    name: 'start-failed-nothing-to-review',
    about: 'The start failed because nothing is left to review, which the reason line says once.',
    async reach(session, page) {
      await startScreen(session);
      await submit(page, 'Start');
      await session.reviewEverything();
      await after(session, () => session.failStart());
    },
  },
  {
    name: 'start-refused',
    about: 'A start posted after a round started in the pane: the notice, over the running round.',
    async reach(session, page) {
      await startScreen(session);
      await refused(session, page, () => session.sendKickoff(), 'Start');
    },
  },
  {
    name: 'working',
    about: 'The agent works on its first turn: Stop waiting, and the rail with Design working.',
    reach: (session) => session.open(),
  },
  {
    name: 'working-reset-open',
    about: "Reset chosen in the masthead's menu: its hint and Confirm reset, in place of the menu.",
    async reach(session, page) {
      await session.open();
      await page.getByRole('button', { name: 'Round menu' }).click();
      await page.getByRole('button', { name: /^Reset this round/ }).click();
    },
  },
  {
    name: 'working-last-answer',
    about: 'The agent works on the answer given in the pane: the previous turn, with Cancel this answer.',
    async reach(session) {
      await session.askQuestion();
      await after(session, () => session.answerInPane());
    },
  },
  {
    name: 'interrupted',
    about: 'The reviewer stopped waiting for the agent: Retry.',
    reach: (session) => after(session, () => session.interrupt()),
  },
  {
    name: 'delivery-failed',
    about: "The prompt of the agent's next turn could not be delivered.",
    reach: (session) => after(session, () => session.failDelivery()),
  },
  {
    name: 'not-started',
    about: 'The agent did not start on the prompt of its next turn.',
    reach: (session) => after(session, () => session.agentDoesNotStart()),
  },
  {
    name: 'delivery-failed-last-answer',
    about: "The prompt with the reviewer's answer could not be delivered: Retry, and the previous turn with Cancel this answer.",
    async reach(session, page) {
      await question(session, page, 1);
      await choose(page, IDLE_OR_FULL);
      await page.getByRole('textbox', { name: 'Comment · optional' }).fill(COMMENT);
      await submit(page, 'Send answer');
      await after(session, () => session.failDelivery());
    },
  },
  {
    name: 'interrupted-uncertain',
    about: "The review was reopened while the turn's prompt was being delivered: whether the agent got it is unknown.",
    reach: (session) => after(session, () => session.reopenWhileSending()),
  },
  {
    name: 'retry-refused',
    about: 'A Retry posted after the round moved on in the pane: the notice.',
    async reach(session, page) {
      await after(session, () => session.failDelivery());
      await refused(session, page, () => session.answerInPane(), 'Retry');
    },
  },
  {
    name: 'earlier-round',
    about: 'Another reviewer saved a newer round: this one says so and offers only Reset.',
    async reach(session) {
      await session.interrupt();
      await after(session, () => session.becomeEarlierRound());
    },
  },
  {
    name: 'reconnecting',
    about: 'The review tool restarts: the quiet line says the page reconnects, and its actions wait.',
    async reach(session, page) {
      await session.interrupt();
      await session.open();
      await page.getByRole('button', { name: 'Retry', exact: true }).waitFor();
      await session.restartReviewer();
      await page.getByText('Reconnecting to the review tool', { exact: false }).waitFor();
    },
  },
  {
    name: 'storage-failed',
    about: "The review tool cannot save the reviewer's rounds: nothing can be done on the page.",
    reach: (session) => after(session, () => session.failStorage()),
  },
  {
    name: 'design',
    about:
      'The round opens on the design of the change: its thesis, four parts led by their figures and tables, and the design map, which ends on question 1.',
    reach: (session) => design(session, 1),
  },
  {
    name: 'design-reopened',
    about: 'The design opened again from question 2, a one-way question: Go to question 2.',
    reach: (session) => design(session, 2),
  },
  {
    name: 'question-1',
    about:
      'The first question, two-way: Context with a table, a diagram and a sketch, Door and Blast radius, four long choices, three citations.',
    reach: (session, page) => question(session, page, 1),
  },
  {
    name: 'question-1-unfolded',
    about: 'The first question with every folded part open: Door, Blast radius and the other two citations.',
    async reach(session, page) {
      await question(session, page, 1);
      await unfold(page);
    },
  },
  {
    name: 'question-1-answer-refused',
    about: 'An answer sent after the question was answered in the pane: the notice.',
    async reach(session, page) {
      await question(session, page, 1);
      await choose(page, MERGE);
      await refused(session, page, () => session.answerInPane(), 'Send answer');
    },
  },
  {
    name: 'working-after-answer',
    about: 'The reviewer sent an answer from the page: the agent works.',
    async reach(session, page) {
      await question(session, page, 1);
      await choose(page, IDLE_OR_FULL);
      await page.getByRole('textbox', { name: 'Comment · optional' }).fill(COMMENT);
      await submit(page, 'Send answer');
    },
  },
  {
    name: 'question-2-blind',
    about:
      "The second question, one-way: the agent's reply to the previous answer, and the choices in a mixed order with the recommendation hidden until the first pick.",
    reach: (session, page) => question(session, page, 2),
  },
  {
    name: 'question-2-blind-picked',
    about:
      "The first Send of a blind question: the recommendation shows, with the line that says the agent picked another choice, the first pick's tag, the comment kept, and Confirm answer.",
    async reach(session, page) {
      await question(session, page, 2);
      await choose(page, DROP);
      await page.getByRole('textbox', { name: 'Comment · optional' }).fill(COMMENT);
      await submit(page, 'Send answer');
    },
  },
  {
    name: 'question-2-blind-same-pick',
    about: 'The first Send picked the choice the agent recommends: the line says that both picked the same choice.',
    async reach(session, page) {
      await question(session, page, 2);
      await choose(page, CLOSE);
      await submit(page, 'Send answer');
    },
  },
  {
    name: 'question-2-cancel-asked',
    about: 'Cancel this answer, opened in the previous turn: the hint and Confirm: cancel my answer.',
    async reach(session, page) {
      await question(session, page, 2);
      await page.getByText('Cancel this answer…').click();
    },
  },
  {
    name: 'question-2-design-map',
    about: 'The design map, open from "Design ▾" on the rail: the thesis, the four parts and a link to the design.',
    async reach(session, page) {
      await question(session, page, 2);
      await page.getByRole('button', { name: 'Design', exact: true }).click();
    },
  },
  {
    name: 'question-2-menu',
    about: "The masthead's ⋯ menu: Copy the round's link, and Reset this round.",
    async reach(session, page) {
      await question(session, page, 2);
      await page.getByRole('button', { name: 'Round menu' }).click();
    },
  },
  {
    name: 'question-2-pick-refused',
    about: 'A pick sent after the question was answered in the pane: the notice.',
    async reach(session, page) {
      await question(session, page, 2);
      await choose(page, DROP);
      await refused(session, page, () => session.answerInPane(), 'Send answer');
    },
  },
  {
    name: 'question-2-answer-cancelled',
    about: 'The question asked again after Cancel answer in the pane: its recommendation shows at once.',
    async reach(session, page) {
      await question(session, page, 2);
      await session.answerInPane();
      await after(session, () => session.cancelAnswerInPane());
    },
  },
  {
    name: 'question-3-diagram-error',
    about:
      'The third question, unfolded: a diagram Mermaid cannot parse, and a citation of a file outside the change.',
    async reach(session, page) {
      await question(session, page, 3);
      await unfold(page);
    },
  },
  {
    name: 'quiz',
    about: 'The conclusion asks its quiz first: the first item of three.',
    reach: (session) => conclusion(session, true),
  },
  {
    name: 'quiz-correct',
    about: 'The reviewer picked the correct answer of the first item: the verdict and its proof.',
    async reach(session, page) {
      await conclusion(session, true);
      await choose(page, QUIZ_RIGHT);
      await submit(page, 'Check');
    },
  },
  {
    name: 'quiz-wrong',
    about: 'The reviewer picked a wrong answer of the second item.',
    reach: quizRightThenWrong,
  },
  {
    name: 'conclusion-quiz-results',
    about: 'The reviewer skipped the last item: the conclusion, with the results of the quiz.',
    async reach(session, page) {
      await quizRightThenWrong(session, page);
      await submit(page, 'Skip the quiz');
    },
  },
  {
    name: 'conclusion',
    about:
      'The conclusion without a quiz: the reply to the last answer, the lead, Your decisions, the rest of the summary, Future work, and the list to be implemented with Implement and the folded reply.',
    reach: (session) => conclusion(session, false),
  },
  {
    name: 'implement-sending',
    about: 'The reviewer pressed Implement: the request is being sent, with its cancel.',
    reach: implementing,
  },
  {
    name: 'implement-sent',
    about: 'The agent received the implementation request.',
    async reach(session, page) {
      await implementing(session, page);
      await after(session, () => session.deliverImplementation());
    },
  },
  {
    name: 'implement-sent-new-round',
    about: 'The agent received the request: "Start a new round…" open, with its confirmation.',
    async reach(session, page) {
      await implementing(session, page);
      await after(session, () => session.deliverImplementation());
      await page.getByRole('button', { name: 'Start a new round…' }).click();
    },
  },
  {
    name: 'conclusion-reply-open',
    about: 'The conclusion with "Not ready? Reply to the agent instead" open.',
    async reach(session, page) {
      await conclusion(session, false);
      await page.getByRole('button', { name: 'Not ready? Reply to the agent instead' }).click();
    },
  },
  {
    name: 'implement-failed',
    about: 'The implementation request could not be delivered: Implement again.',
    async reach(session, page) {
      await implementing(session, page);
      await after(session, () => session.failDelivery());
    },
  },
  {
    name: 'implement-not-started',
    about: 'The agent did not start on the implementation request: Retry only.',
    async reach(session, page) {
      await implementing(session, page);
      await after(session, () => session.agentDoesNotStart());
    },
  },
  {
    name: 'implement-cancelled',
    about: 'The reviewer cancelled the request before it was sent: Implement again.',
    async reach(session, page) {
      await implementing(session, page);
      await submit(page, 'Cancel the implementation request');
    },
  },
  {
    name: 'implement-paused',
    about: 'The review was reopened before the request went out: send the saved request, or a new one.',
    async reach(session, page) {
      await implementing(session, page);
      await after(session, () => session.reopenBeforeSending());
    },
  },
  {
    name: 'implement-paused-editing',
    about: 'The saved request, with "Edit before sending" open: the list to edit and Send a new request.',
    async reach(session, page) {
      await implementing(session, page);
      await after(session, () => session.reopenBeforeSending());
      await page.getByRole('button', { name: 'Edit before sending' }).click();
    },
  },
  {
    name: 'implement-unknown',
    about: 'The review was reopened while the request was being delivered: whether the agent got it is unknown.',
    async reach(session, page) {
      await implementing(session, page);
      await after(session, () => session.reopenWhileSending());
    },
  },
  {
    name: 'implement-refused',
    about: 'An Implement posted after the request was sent from the pane: the notice.',
    async reach(session, page) {
      await conclusion(session, false);
      await refused(session, page, () => session.implementInPane(), IMPLEMENT);
    },
  },
];
