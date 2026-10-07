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
  /**
   * Widths the state is shot at besides the run's, for a state whose layout depends on a wide
   * window (a stage with no panel, centred in the frame).
   */
  extraWidths?: number[];
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

/**
 * Opens the meter on the masthead's hairline with a click a third of the way across, as a
 * pointer would, and moves the pointer away: the window stays pinned.
 */
async function openMeter(page: Page): Promise<void> {
  const strip = page.getByRole('button', { name: /^Lines reviewed/ });
  const box = await strip.boundingBox();
  await strip.click({ position: { x: (box?.width ?? 0) * 0.52, y: 4 } });
  await page.mouse.move(0, 0);
}

/** Selects the choice `name` of the question or the quiz item on the page. */
async function choose(page: Page, name: string): Promise<void> {
  await page.getByRole('radio', { name }).check();
}

/**
 * Waits until the open chat has come in and the page has made room for it: no transition runs
 * (the chat's rise with the masthead runs on the page's scroll, not in time), and a diagram has
 * fitted the column again.
 */
async function chatSettled(page: Page): Promise<void> {
  await page.getByRole('complementary', { name: 'Conversation with the agent' }).waitFor();
  await page.waitForFunction(
    () =>
      document.querySelector('.chat.open') !== null &&
      document.getAnimations().every((animation) => animation.timeline !== document.timeline || animation.playState !== 'running') &&
      [...document.querySelectorAll('figure:not(.scrolls) .drawing > svg')].every(
        (drawing) => drawing.getBoundingClientRect().width <= (drawing.closest('figure')?.clientWidth ?? Infinity),
      ),
  );
}

/**
 * Sends `text` from the chat, which opens first when it is closed, and waits until the chat shows
 * the message.
 */
async function chatTo(page: Page, text: string): Promise<void> {
  const chat = page.getByRole('complementary', { name: 'Conversation with the agent' });
  if (!(await chat.isVisible())) await page.getByRole('button', { name: 'Talk to the agent' }).click();
  await chat.getByRole('textbox', { name: 'Message to the agent' }).fill(text);
  await chat.getByRole('button', { name: 'Send', exact: true }).click();
  await chat.getByRole('article', { name: 'Your message' }).filter({ hasText: text.replaceAll('`', '') }).waitFor();
}

/**
 * Selects the last sentence of paragraph `index` of the question's Context, as the reviewer
 * selects a passage.
 */
async function selectPassage(page: Page, index: number): Promise<void> {
  await page.locator('.explanation > p').nth(index).waitFor();
  await page.evaluate((index) => {
    const paragraph = document.querySelectorAll('.explanation > p')[index];
    if (!paragraph) throw new Error(`no paragraph ${index}`);
    // The paragraph's last stretch of plain text that holds a sentence.
    const walker = document.createTreeWalker(paragraph, NodeFilter.SHOW_TEXT);
    let text: Text | null = null;
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      if ((node.textContent ?? '').trim().length > 24) text = node as Text;
    }
    if (!text) throw new Error(`no text in paragraph ${index}`);
    const content = text.textContent ?? '';
    const start = content.lastIndexOf('. ', content.length - 3);
    const range = document.createRange();
    range.setStart(text, start < 0 ? 0 : start + 2);
    range.setEnd(text, content.trimEnd().length);
    document.getSelection()?.removeAllRanges();
    document.getSelection()?.addRange(range);
  }, index);
}

/** Selects the last sentence of paragraph `index` of the Context, then adds it to the chat. */
async function quote(page: Page, index: number): Promise<void> {
  // On a phone the open chat covers the page: the reviewer closes it to select.
  if (await page.locator('.chat-scrim').isVisible()) {
    await page.getByRole('button', { name: 'Close the conversation' }).click();
  }
  await selectPassage(page, index);
  await page.getByRole('button', { name: 'Add to chat' }).click();
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
const QUIZ_LAST_RIGHT = 'At the twentieth reply';

const IMPLEMENT = 'Implement';

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
    about:
      'The agent works on its first turn: Stop waiting, and the rail with Design working; with no panel, the card is centred in the frame, also in a 2000-pixel window.',
    reach: (session) => session.open(),
    extraWidths: [2000],
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
    about:
      'The agent works on the answer given in the pane: the card with the time since it was sent, the answered question, and in the panel the answer with Stop waiting and Cancel this answer.',
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
    about:
      "The prompt with the reviewer's answer could not be delivered: the card, the answered question, and in the panel the answer with Retry and Cancel this answer.",
    async reach(session, page) {
      await question(session, page, 1);
      await choose(page, IDLE_OR_FULL);
      await page.getByRole('textbox', { name: 'Comment · optional' }).fill(COMMENT);
      await submit(page, 'Send answer');
      await after(session, () => session.failDelivery());
    },
  },
  {
    name: 'stopped-last-answer',
    about: 'The reviewer stopped waiting for the answer: the turn is paused, and the panel offers Retry.',
    async reach(session) {
      await session.askQuestion();
      await session.answerInPane();
      await after(session, () => session.interrupt());
    },
  },
  {
    name: 'not-started-last-answer',
    about: 'The agent did not start on the answer: the warning, and Retry in the panel.',
    async reach(session) {
      await session.askQuestion();
      await session.answerInPane();
      await after(session, () => session.agentDoesNotStart());
    },
  },
  {
    name: 'uncertain-last-answer',
    about:
      "The review was reopened while the answer's prompt was being delivered: check the agent's pane, and Retry as a secondary action in the panel.",
    async reach(session) {
      await session.askQuestion();
      await session.answerInPane();
      await after(session, () => session.reopenWhileSending());
    },
  },
  {
    name: 'interrupted-uncertain',
    about: "The review was reopened while the turn's prompt was being delivered: whether the agent got it is unknown.",
    reach: (session) => after(session, () => session.reopenWhileSending()),
  },
  {
    name: 'retry-refused',
    about: 'A Retry posted after the round moved on in the pane: the notice, over the question the agent asked next.',
    async reach(session, page) {
      await ask(session, 1);
      await session.answerInPane();
      await after(session, () => session.failDelivery());
      await refused(session, page, () => session.askQuestion(), 'Retry');
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
    about: 'The reviewer sent an answer from the page: the agent works on it, the time since it was sent counts on.',
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
    name: 'question-2-prepared',
    about:
      'The second question from a turn run-ahead prepared while the reviewer was thinking: the previous turn says so.',
    async reach(session) {
      await session.askQuestion();
      await session.answerInPane();
      await session.askPreparedQuestion();
      await session.open();
    },
  },
  {
    name: 'question-2-not-prepared',
    about:
      'The second question from a turn the agent took itself while run-ahead watched the question: the previous turn says, quietly, why no prepared turn was used.',
    async reach(session) {
      await session.askQuestion();
      await session.answerInPane();
      await session.askNotPreparedQuestion();
      await session.open();
    },
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
    name: 'waiting-after-blind-answer',
    about:
      'The reviewer confirmed another choice than the first pick of a blind question, with a comment: the answered question with its chips, and the answer tagged "changed after your first pick".',
    async reach(session, page) {
      await question(session, page, 2);
      await choose(page, DROP);
      await page.getByRole('textbox', { name: 'Comment · optional' }).fill(COMMENT);
      await submit(page, 'Send answer');
      await choose(page, CLOSE);
      await submit(page, 'Confirm answer');
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
    about: "The masthead's ⋯ menu: Copy the round's link, Open the agent's conversation, and Reset this round.",
    async reach(session, page) {
      await question(session, page, 2);
      await page.getByRole('button', { name: 'Round menu' }).click();
    },
  },
  {
    name: 'question-1-earlier',
    about:
      'Question 1 opened again from the rail at question 3, read only: the question as it was asked, the answer, what it marked, what the agent recorded, and the way back.',
    async reach(session, page) {
      await question(session, page, 3);
      // The rail's link to question 1, which a phone's rail hides: the address it goes to.
      await page.evaluate(() => {
        location.hash = '#question-1';
      });
      await page.getByRole('region', { name: 'Question 1 · answered' }).waitFor();
    },
  },
  {
    name: 'question-2-swipe',
    about:
      'On a phone, a drag to the right past the threshold, held: question 1 slides in beside question 2, its chip fills and the left edge lights.',
    async reach(session, page) {
      await question(session, page, 2);
      const headline = page.getByRole('region', { name: 'Question 2' }).getByRole('heading').first();
      const box = await headline.boundingBox();
      if (!box) return;
      const y = box.y + Math.min(box.height / 2, 20);
      await page.mouse.move(box.x + 40, y);
      await page.mouse.down();
      await page.mouse.move(box.x + 80, y);
      await page.mouse.move(box.x + 160, y);
    },
  },
  {
    name: 'question-2-chat',
    about:
      "The chat, opened from its bubble at the left of the masthead: a message under Q2 and the agent's reply, a message quoting a passage that the agent is answering, and a quote waiting in the composer. From 1424 pixels (here 1440 and 2000) it is a column of its own at the window's left, beside the question and its panel; at 1280 it lies over the left of the page and leaves the panel in view; on a phone it is a sheet.",
    extraWidths: [1440, 2000],
    async reach(session, page) {
      await question(session, page, 2);
      await chatTo(page, 'Who else calls `flush`?');
      await session.agentReplies();
      await page.getByRole('article', { name: 'Reply from the agent' }).waitFor();
      await quote(page, 0);
      await chatTo(page, 'Why not wait? What is lost if the notification never arrives?');
      await quote(page, 1);
      await chatSettled(page);
    },
  },
  {
    name: 'question-2-chat-wide-table',
    about:
      "The agent's reply holds a table of five columns and a block of long lines: beside the page (here 1440 and 2000) the chat widens to fit it, up to what leaves the reading column its narrowest width beside the panel, and what still does not fit scrolls in its own frame; at 1280, a drawer, and on a phone, a sheet, it keeps its width.",
    extraWidths: [1440, 2000],
    async reach(session, page) {
      await question(session, page, 2);
      await chatTo(page, 'Who else calls `flush`?');
      await session.agentRepliesWide();
      await page.getByRole('article', { name: 'Reply from the agent' }).waitFor();
      await chatSettled(page);
    },
  },
  {
    name: 'question-2-chat-select',
    about: 'A passage of the question selected: the one option above it, Add to chat.',
    extraWidths: [1440, 2000],
    async reach(session, page) {
      await question(session, page, 2);
      await selectPassage(page, 1);
      await page.getByRole('button', { name: 'Add to chat' }).waitFor();
    },
  },
  {
    name: 'question-2-chat-unread',
    about:
      "The agent replied while the chat was closed: the unread count on the bubble, at the left of the masthead on a desktop, and in the tab's title.",
    extraWidths: [1440, 2000],
    async reach(session, page) {
      await question(session, page, 2);
      await chatTo(page, 'Who else calls `flush`?');
      await page.getByRole('button', { name: 'Close the conversation' }).click();
      await session.agentReplies();
      await page.getByRole('button', { name: 'Talk to the agent' }).getByText('1').waitFor();
    },
  },
  {
    name: 'question-2-chat-not-delivered',
    about: 'A message of the chat did not reach the agent: the failure, with Retry.',
    extraWidths: [1440, 2000],
    async reach(session, page) {
      await question(session, page, 2);
      await chatTo(page, 'Who else calls `flush`?');
      await session.messagesNotDelivered();
      await page.getByRole('alert').filter({ hasText: 'did not reach the agent' }).waitFor();
    },
  },
  {
    name: 'question-2-meter',
    about:
      "The meter on the masthead's hairline, open: the bar split by who marked the lines, and the window with the totals, the legend and a row for each of the change's 43 files, as wide as the long paths need within its limit, its list scrolling inside it.",
    extraWidths: [1440, 2000],
    async reach(session, page) {
      await question(session, page, 2);
      await openMeter(page);
    },
  },
  {
    name: 'question-2-meter-by-hand',
    about: 'The meter, open after the reviewer marked lines by hand during the round: their own part and legend row.',
    extraWidths: [1440, 2000],
    async reach(session, page) {
      await question(session, page, 2);
      await after(session, () => session.markByHand());
      await openMeter(page);
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
    name: 'question-wide-diagram',
    about:
      'A question whose sequence diagram has seven participants: shrunk to show whole, widened to the reading column, with Open large.',
    async reach(session, page) {
      await session.askQuestion(WIDE_DIAGRAM);
      await session.open();
      await submit(page, 'Go to question 1');
    },
  },
  {
    name: 'question-wide-diagram-chat',
    about:
      'The same question with the chat open: beside the page from 1424 pixels, the diagram shrinks again to the narrower reading column, with Open large.',
    extraWidths: [1440, 2000],
    async reach(session, page) {
      await session.askQuestion(WIDE_DIAGRAM);
      await session.open();
      await submit(page, 'Go to question 1');
      await page.getByRole('button', { name: 'Talk to the agent' }).click();
      await chatSettled(page);
    },
  },
  {
    name: 'quiz',
    about: 'The conclusion asks its quiz first: the first item of three, its answers and Check in the panel.',
    reach: (session) => conclusion(session, true),
  },
  {
    name: 'quiz-correct',
    about:
      'The reviewer picked the correct answer of the first item: the verdict, the marked answers and Next question in the panel, the proof on the desk.',
    async reach(session, page) {
      await conclusion(session, true);
      await choose(page, QUIZ_RIGHT);
      await submit(page, 'Check');
    },
  },
  {
    name: 'quiz-wrong',
    about: 'The reviewer picked a wrong answer of the second item: the wrong pick and the correct answer, each marked.',
    reach: quizRightThenWrong,
  },
  {
    name: 'quiz-last-checked',
    about: 'The reviewer checked the last item: Show the conclusion, and no Skip the quiz.',
    async reach(session, page) {
      await quizRightThenWrong(session, page);
      await submit(page, 'Next question');
      await choose(page, QUIZ_LAST_RIGHT);
      await submit(page, 'Check');
    },
  },
  {
    name: 'quiz-checked-earlier',
    about:
      'During the quiz, the reviewer went back to the first item, checked already: its verdict, marked answers and proof, read only, with Next question back to the second.',
    async reach(session, page) {
      await quizRightThenWrong(session, page);
      await submit(page, 'Previous');
    },
  },
  {
    name: 'conclusion-quiz-results',
    about: 'The reviewer skipped the last item: the conclusion, with the score of the quiz in its panel and the quiz done on the rail.',
    async reach(session, page) {
      await quizRightThenWrong(session, page);
      await submit(page, 'Skip the quiz');
    },
  },
  {
    name: 'conclusion-quiz-answers',
    about:
      'The answered quiz opened from the score: its first item, read only, with the score and the way back to the conclusion; the rail shows the quiz in view.',
    async reach(session, page) {
      await quizRightThenWrong(session, page);
      await submit(page, 'Skip the quiz');
      await submit(page, 'See the answers');
    },
  },
  {
    name: 'quiz-answered-skipped',
    about: 'The answered quiz at the item the reviewer skipped, opened from its dot: Skipped., the correct answer, the proof.',
    async reach(session, page) {
      await quizRightThenWrong(session, page);
      await submit(page, 'Skip the quiz');
      await submit(page, 'See the answers');
      await submit(page, 'Question 3, skipped');
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
    about:
      'The conclusion after "Not ready? Reply to the agent instead": the chat, open at the left of the window, beside the conclusion and its list from 1424 pixels (here 1440 and 2000).',
    extraWidths: [1440, 2000],
    async reach(session, page) {
      await conclusion(session, false);
      await page.getByRole('button', { name: 'Not ready? Reply to the agent instead' }).click();
      await chatSettled(page);
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

/** A question whose Context draws a sequence diagram of seven participants, wider than the
 * reading column at its natural size. */
const WIDE_DIAGRAM = {
  id: 'flush-on-close',
  version: 1,
  topic: 'flush',
  text: 'Who waits for whom when the pane closes with replies still queued?',
  rationale: [
    'Closing the pane runs through seven parts of the reviewer, one after the other:',
    '',
    '```mermaid',
    'sequenceDiagram',
    '  autonumber',
    '  participant reviewer as Reviewer',
    '  participant pane as Pane',
    '  participant thread as Thread file',
    '  participant queue as ReplyQueue',
    '  participant policy as FlushPolicy',
    '  participant relay as Agent link',
    '  participant agent as Agent',
    '  reviewer->>pane: close the pane',
    '  pane->>queue: flush(Closing)',
    '  queue->>policy: may it go out now?',
    '  policy-->>queue: yes, closing',
    '  queue->>relay: notify_batch(replies)',
    '  relay->>agent: one notification',
    '  pane->>thread: save every thread',
    '  thread-->>pane: saved',
    '```',
  ].join('\n'),
  visual: null,
  alternatives: [
    { id: 'wait', text: 'Wait for the agent link before closing', outcome: 'needs_follow_up', recommendation: null },
    { id: 'close', text: 'Close at once, as the change does', outcome: 'accepted', recommendation: 'Nothing is lost: each reply is saved first.' },
  ],
  evidence: [{ path: 'src/threads/reply.rs', side: 'new', lines: { first_line: 20, last_line: 27 }, notes: 'Closing flushes the queue, then saves.' }],
  assessments: null,
};
