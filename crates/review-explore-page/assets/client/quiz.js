// The conclusion's quiz (docs/design/explore-page/README.md, "4. Quiz"; design review, finding
// 18), one item at a time, on the desk: the item as the headline under "QUIZ · QUESTION 2 OF 3"
// and a dot for each item; in the panel the answers and Check. After Check, the panel opens with
// the verdict, each answer carries its mark (a glyph and a word: ✓ Correct, ✗ Your pick), the
// proof joins the reading column, and the panel offers one next step, Next question (Show the
// conclusion after the last item), with a quiet Skip the quiz.
//
// One screen draws the quiz, in two modes. While the quiz asks, it is the round's stage, before
// the conclusion. Once it is over, the answered quiz opens read only from the rail's quiz step or
// from the score in the conclusion's panel (`#quiz-N`, route.js): each item with the reviewer's
// pick, the correct answer, the verdict and the proof, under the score and the way back to the
// conclusion.
//
// Previous, Next question and the dots in the eyebrow move between the items; on a phone the two
// buttons sit in a bar at the bottom of the window. While the quiz asks, they reach the items
// already checked, read only, and the next one to check, never one beyond it; once it is over,
// every item. Which item shows while the quiz asks is the page's own state (page.js), and the
// rail names that item (railShowing).

/** @import { QuizView, QuizItemView, QuizAnswerView, RailStep } from "./types.ts" */
/** @import { Turn } from "./turn.js" */

import { choiceCards, plainChoices } from './choices.js';
import { citation } from './citations.js';
import { codeSpans, h, keyOf, Region } from './dom.js';
import { STAGE } from './route.js';
import { turnStrip } from './turn.js';

/**
 * The quiz item the page shows while the quiz asks, from 0, in the conclusion of the request
 * `conclusion`.
 * @typedef {{ conclusion: string, item: number }} QuizAt
 */

/**
 * Which item the quiz screen shows, in which mode, and where its buttons lead.
 * @typedef {object} QuizShowing
 * @property {number} shown the item shown, from 0
 * @property {boolean} asking whether the quiz asks for answers: the screen is the round's stage;
 *   else it shows the answered quiz, read only
 * @property {(item: number) => void} open shows the item `item`, from 0
 * @property {() => void} done leaves the quiz for the conclusion, once every item is checked
 */

export class QuizScreen {
  constructor() {
    this.element = h('section', { class: 'quiz desk', id: 'quiz', 'aria-labelledby': 'quiz-label' });
    // The previous turn, above the quiz it led to, while the quiz is the round's stage.
    this.turn = new Region(this.element, 'turn');
    this.head = new Region(this.element, 'head');
    this.panel = new Region(this.element, 'panel');
    // After the panel, as the question's citations: on a phone the proof follows the verdict.
    this.proof = new Region(this.element, 'proof');
    /** The item shown, from 0. @type {number | null} */
    this.shown = null;
  }

  /**
   * @param {QuizView} quiz
   * @param {string} conclusion the request of the turn that posted the conclusion
   * @param {Turn} turn the turn that led to the conclusion
   * @param {QuizShowing} showing
   */
  update(quiz, conclusion, turn, showing) {
    const { shown, asking } = showing;
    const item = /** @type {QuizItemView} */ (quiz.items[shown]);
    const checked = item.picked_correct !== null;
    this.turn.show(asking ? keyOf(turn) : null, () => turnStrip(turn));
    const reach = quiz.items.map((_, index) => itemOpens(quiz, index, asking));
    const marks = quiz.items.map((each) => each.picked_correct);
    this.head.show(keyOf([shown, item.question, marks, reach, asking, quiz.skipped]), () => head(quiz, showing, reach));
    this.panel.show(keyOf([conclusion, shown, quiz, asking]), () => panel(quiz, item, conclusion, showing, reach));
    const proven = checked || !asking;
    this.proof.show(keyOf([shown, proven ? item.proof : null]), () => (proven ? proof(item) : null));
    if (this.shown !== null && this.shown !== shown) {
      // The next item starts at its headline, where the reviewer reads it from.
      if (this.element.getBoundingClientRect().top < 0) this.element.scrollIntoView({ block: 'start' });
      // The control that moved to another item keeps the focus (dom.js) while it can still act;
      // else the reviewer goes on from the headline.
      const headline = this.element.querySelector('.quiz-item');
      if (!this.element.contains(document.activeElement) && headline instanceof HTMLElement) {
        headline.focus({ preventScroll: true });
      }
    }
    this.shown = shown;
  }
}

/**
 * Whether the item `index` (from 0) opens: any item once the quiz is over; while it asks, an item
 * already checked, and the next one to check.
 * @param {QuizView} quiz
 * @param {number} index
 * @param {boolean} asking
 */
export function itemOpens(quiz, index, asking) {
  const item = quiz.items[index];
  if (!item) return false;
  return !asking || item.picked_correct !== null || index === quiz.next;
}

/**
 * Whether the rail's quiz step opens the answered quiz: once the reviewer answered or skipped
 * every item, unless the reviewer skipped the quiz without answering any.
 * @param {RailStep} step
 */
export function quizOpens(step) {
  return (
    step.step.kind === 'quiz' &&
    step.state.kind === 'done' &&
    step.step.stage.kind === 'scored' &&
    step.step.stage.answered > 0
  );
}

/**
 * The rail as it stands while the page shows the item `shown` (from 0) of a quiz of `items`: the
 * quiz is the current step, at that item, even right after the reviewer checked the last one,
 * until the reviewer moves on.
 * @param {RailStep[]} rail
 * @param {number} shown
 * @param {number} items
 * @returns {RailStep[]}
 */
export function railShowing(rail, shown, items) {
  return rail.map((step) => {
    if (step.step.kind === 'quiz') {
      return {
        step: { kind: 'quiz', stage: { kind: 'running', item: shown + 1, items } },
        state: { kind: 'current', working: false },
      };
    }
    if (step.step.kind === 'conclusion') return { ...step, state: { kind: 'later' } };
    return step;
  });
}

/**
 * "QUIZ · QUESTION 2 OF 3" with a dot for each item, the item as the headline, and the line that
 * says the quiz marks nothing; before Check, that its proof shows after.
 * @param {QuizView} quiz
 * @param {QuizShowing} showing
 * @param {boolean[]} reach which items open
 */
function head(quiz, showing, reach) {
  const item = /** @type {QuizItemView} */ (quiz.items[showing.shown]);
  const before = showing.asking && item.picked_correct === null;
  return h(
    'header',
    { class: 'quiz-head' },
    h(
      'p',
      { class: 'eyebrow quiz-eyebrow' },
      h('span', { id: 'quiz-label' }, `Quiz · Question ${item.number} of ${quiz.items.length}`),
      dots(quiz, showing, reach),
    ),
    h('h2', { class: 'quiz-item', tabindex: -1 }, codeSpans(item.question)),
    h(
      'p',
      { class: 'hint' },
      'The quiz checks what you took from the design; nothing here marks lines.',
      before ? ' Its proof shows once you check your answer.' : null,
    ),
  );
}

/** What became of an item: its pick was correct, wrong, or there is no pick yet.
 * @typedef {'correct' | 'wrong' | 'open'} Outcome */

/** How the page says each outcome: the dot's class and words, the verdict's class and words. */
const OUTCOMES = {
  correct: { dot: 'good', dotWords: 'correct', tone: 'good', verdict: 'Correct.' },
  wrong: { dot: 'bad', dotWords: 'wrong', tone: 'bad', verdict: 'Not quite.' },
  open: { dot: 'pending', dotWords: 'not answered yet', tone: 'open', verdict: 'Not answered.' },
};

/** @param {QuizItemView} item
 * @returns {Outcome} */
function outcomeOf(item) {
  if (item.picked_correct === null) return 'open';
  return item.picked_correct ? 'correct' : 'wrong';
}

/**
 * A dot for each item, which opens it: green when its pick was correct, red when wrong, an
 * outline when not answered, ringed for the item shown. The colours say it at a glance; each
 * dot's name says it in words.
 * @param {QuizView} quiz
 * @param {QuizShowing} showing
 * @param {boolean[]} reach which items open
 */
function dots(quiz, showing, reach) {
  const unanswered = !showing.asking && quiz.skipped ? 'skipped' : OUTCOMES.open.dotWords;
  return h(
    'span',
    { class: 'quiz-dots', role: 'group', 'aria-label': 'Quiz questions' },
    quiz.items.map((item, index) => {
      const outcome = outcomeOf(item);
      const name = `Question ${item.number}, ${outcome === 'open' ? unanswered : OUTCOMES[outcome].dotWords}`;
      return h(
        'button',
        {
          class: `dot ${OUTCOMES[outcome].dot}`,
          type: 'button',
          'aria-label': name,
          title: name,
          'aria-current': index === showing.shown ? 'step' : null,
          disabled: !reach[index],
          'data-focus-key': `quiz-dot-${item.number}`,
          onclick: () => {
            if (index !== showing.shown) showing.open(index);
          },
        },
        h('span', { class: 'disc', 'aria-hidden': 'true' }),
      );
    }),
  );
}

/**
 * The panel: what the item asks or what became of it, then Previous and the next step, then Skip
 * the quiz while there is a quiz to skip.
 * @param {QuizView} quiz
 * @param {QuizItemView} item
 * @param {string} conclusion
 * @param {QuizShowing} showing
 * @param {boolean[]} reach which items open
 */
function panel(quiz, item, conclusion, showing, reach) {
  const { asking } = showing;
  const checked = item.picked_correct !== null;
  return h(
    'section',
    { class: 'quiz-panel panel', 'aria-label': `Your answer to quiz question ${item.number}` },
    asking ? (checked ? checkedItem(item) : pick(item, conclusion)) : answeredItem(quiz, item),
    moves(quiz, showing, reach),
    asking && quiz.next !== null ? skipForm(conclusion) : null,
  );
}

/**
 * Before Check: the answers as choice cards, and Check, which waits for a pick.
 * @param {QuizItemView} item
 * @param {string} conclusion
 */
function pick(item, conclusion) {
  const answers = plainChoices(item.answers.map((answer) => ({ id: String(answer.index), text: answer.text })));
  return h(
    'form',
    { class: 'quiz-pick', 'data-method': 'quiz', 'data-requires': 'answer' },
    h('input', { type: 'hidden', name: 'conclusion', value: conclusion }),
    h('input', { type: 'hidden', name: 'item', value: item.number - 1 }),
    choiceCards(answers, { name: 'answer', legend: 'Answers' }),
    h('button', { class: 'button primary block', type: 'submit' }, 'Check'),
  );
}

/**
 * An item checked while the quiz asks: the verdict, which the page brings into view after Check,
 * then the answers with their marks. Nothing changes the pick any more.
 * @param {QuizItemView} item
 */
function checkedItem(item) {
  return [verdict(item, { role: 'status', tabindex: -1, 'data-shows': 'quiz' }), answerList(item, 'quiz-answers-title')];
}

/**
 * An item of the answered quiz: the score with the way back to the conclusion, the verdict, or
 * "Skipped." for an item the reviewer skipped, then the answers with their marks.
 * @param {QuizView} quiz
 * @param {QuizItemView} item
 */
function answeredItem(quiz, item) {
  return [
    quizScore(quiz, { href: STAGE, words: 'Back to the conclusion' }),
    verdict(item, {}, quiz.skipped ? 'Skipped.' : undefined),
    answerList(item, 'quiz-answers-title'),
  ];
}

/**
 * Previous, and the next step: Next question, or Show the conclusion once the reviewer checked
 * the last item while the quiz asks. Next question is the primary action on an item checked
 * while the quiz asks. Each is inactive where it leads nowhere.
 * @param {QuizView} quiz
 * @param {QuizShowing} showing
 * @param {boolean[]} reach which items open
 */
function moves(quiz, showing, reach) {
  const { shown, asking } = showing;
  const item = /** @type {QuizItemView} */ (quiz.items[shown]);
  const checked = item.picked_correct !== null;
  const conclude = asking && checked && quiz.next === null && shown === quiz.items.length - 1;
  const following = reach[shown + 1] === true;
  return h(
    'nav',
    { class: 'quiz-moves', 'aria-label': 'Move between quiz questions' },
    h(
      'button',
      {
        class: 'button secondary',
        type: 'button',
        disabled: shown === 0,
        'data-focus-key': 'quiz-previous',
        onclick: () => showing.open(shown - 1),
      },
      h('span', { class: 'arrow back', 'aria-hidden': 'true' }, '←'),
      'Previous',
    ),
    h(
      'button',
      {
        class: `button ${asking && checked ? 'primary' : 'secondary'}`,
        type: 'button',
        disabled: !conclude && !following,
        'data-focus-key': 'quiz-next',
        onclick: () => (conclude ? showing.done() : showing.open(shown + 1)),
      },
      conclude ? 'Show the conclusion' : 'Next question',
      h('span', { class: 'arrow', 'aria-hidden': 'true' }, '→'),
    ),
  );
}

/** Skip the quiz: the conclusion shows, with the items left unanswered as skipped.
 * @param {string} conclusion */
function skipForm(conclusion) {
  return h(
    'form',
    { class: 'quiz-skip', 'data-method': 'quiz-skip' },
    h('input', { type: 'hidden', name: 'conclusion', value: conclusion }),
    h('button', { class: 'button quiet', type: 'submit' }, 'Skip the quiz'),
  );
}

/**
 * The verdict on an item: "Correct." or "Not quite.", with why; for an item without a pick,
 * `unanswered` ("Skipped." once the reviewer skipped the rest of the quiz). Its glyph comes
 * from the stylesheet; its words say it all.
 * @param {QuizItemView} item
 * @param {Record<string, string | number>} [props]
 * @param {string} [unanswered]
 */
function verdict(item, props = {}, unanswered = OUTCOMES.open.verdict) {
  const kind = outcomeOf(item);
  const outcome = OUTCOMES[kind];
  const words = kind === 'open' ? unanswered : outcome.verdict;
  return h('p', { class: `verdict ${outcome.tone}`, ...props }, h('span', {}, h('strong', {}, words), ' ', codeSpans(item.why)));
}

/**
 * "ANSWERS", and each answer as a card with its mark: the correct one in green with ✓ and
 * Correct, a wrong pick in red with ✗ and Your pick, the others muted.
 * @param {QuizItemView} item
 * @param {string} id names the list, by its eyebrow
 */
function answerList(item, id) {
  return [
    h('p', { class: 'eyebrow', id }, 'Answers'),
    h('ol', { class: 'quiz-answers', 'aria-labelledby': id }, item.answers.map(answerCard)),
  ];
}

/** An answer after Check: a choice card with no radio, its mark in the radio's place.
 * @param {QuizAnswerView} answer */
function answerCard(answer) {
  const tone = answer.correct ? 'correct' : answer.picked ? 'wrong' : 'other';
  return h(
    'li',
    { class: `choice quiz-answer ${tone}` },
    h('span', { class: 'mark', 'aria-hidden': 'true' }),
    h('span', { class: 'choice-text' }, codeSpans(answer.text)),
    answer.picked || answer.correct
      ? h(
          'span',
          { class: 'choice-tags' },
          answer.picked ? h('span', { class: 'tag accent' }, 'Your pick') : null,
          answer.correct ? h('span', { class: 'tag good' }, 'Correct') : null,
        )
      : null,
  );
}

/**
 * "PROOF": the lines that prove the correct answer, each with its note.
 * @param {QuizItemView} item
 */
function proof(item) {
  const id = `quiz-${item.number}-proof`;
  return h(
    'section',
    { class: 'proof', 'aria-labelledby': id },
    h('h3', { class: 'eyebrow', id }, 'Proof'),
    item.proof.map((each, index) => citation(each, `${id}-${index + 1}`)),
  );
}

/**
 * The quiz's score, which matters right at the Implement decision: warn below two-thirds, good
 * from there, then a link: from the conclusion's panel to the answered quiz, and from the
 * answered quiz back to the conclusion.
 * @param {QuizView} quiz
 * @param {{ href: string, words: string }} link
 */
export function quizScore(quiz, link) {
  const items = quiz.items.length;
  const missed = quiz.picked - quiz.correct_picks;
  const unanswered = items - quiz.picked;
  const questions = (/** @type {number} */ count) => `${count} ${count === 1 ? 'question' : 'questions'}`;
  const words = [
    missed > 0 ? `You missed ${questions(missed)}.` : null,
    unanswered > 0 ? `You ${quiz.skipped ? 'skipped' : 'did not answer'} ${questions(unanswered)}.` : null,
  ].filter((part) => part !== null);
  const tone = quiz.correct_picks * 3 >= items * 2 ? 'good' : 'warn';
  return h(
    'p',
    { class: `quiz-banner ${tone}` },
    h('strong', {}, `Quiz ${quiz.correct_picks} of ${items}`),
    ' ',
    words.length > 0 ? words.join(' ') : 'Every answer was correct.',
    ' ',
    h('a', { href: link.href }, link.words),
  );
}
