// The conclusion's quiz (docs/design/explore-page/README.md, "4. Quiz"; design review, finding
// 18), before the conclusion, one item at a time, on the desk: the item as the headline under
// "QUIZ · QUESTION 2 OF 3" and a dot for each item; in the panel the answers and Check. After
// Check, the panel opens with the verdict, each answer carries its mark (a glyph and a word: ✓
// Correct, ✗ Your pick), the proof joins the reading column, and the panel offers one next step,
// Next question (Show the conclusion after the last item), with a quiet Skip the quiz. Once every
// item is answered or skipped, the results fold beside the conclusion (quizResults).
//
// Which item shows is the page's own state: after a pick, the item just answered shows until the
// reviewer moves on, and the rail names that item (railShowing).

/** @import { QuizView, QuizItemView, QuizAnswerView, RailStep } from "./types.ts" */
/** @import { Turn } from "./turn.js" */

import { choiceCards } from './choices.js';
import { citation } from './citations.js';
import { disclosure, openDisclosure } from './disclosure.js';
import { h, keyOf, Region } from './dom.js';
import { turnStrip } from './turn.js';

export class QuizScreen {
  constructor() {
    this.element = h('section', { class: 'quiz desk', id: 'quiz', 'aria-labelledby': 'quiz-label' });
    // The previous turn, above the quiz it led to.
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
   * @param {number} shown the item shown, from 0
   * @param {Turn} turn the turn that led to the conclusion
   * @param {() => void} next moves on from an answered item
   */
  update(quiz, conclusion, shown, turn, next) {
    const item = /** @type {QuizItemView} */ (quiz.items[shown]);
    const checked = item.picked_correct !== null;
    const asks = quiz.next !== null;
    this.turn.show(keyOf(turn), () => turnStrip(turn));
    const marks = quiz.items.map((each) => each.picked_correct);
    this.head.show(keyOf([shown, item.question, marks]), () => head(quiz, item));
    this.panel.show(keyOf([conclusion, item, asks]), () =>
      checked ? answeredPanel(item, conclusion, asks, next) : pickPanel(item, conclusion, asks),
    );
    this.proof.show(keyOf([shown, checked ? item.proof : null]), () => (checked ? proof(item) : null));
    // The next item starts at its headline, where the reviewer reads it from.
    if (this.shown !== null && this.shown !== shown) {
      const headline = this.element.querySelector('.quiz-item');
      if (headline instanceof HTMLElement) {
        if (this.element.getBoundingClientRect().top < 0) this.element.scrollIntoView({ block: 'start' });
        headline.focus({ preventScroll: true });
      }
    }
    this.shown = shown;
  }
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
 * @param {QuizItemView} item
 */
function head(quiz, item) {
  const checked = item.picked_correct !== null;
  return h(
    'header',
    { class: 'quiz-head' },
    h(
      'p',
      { class: 'eyebrow quiz-eyebrow' },
      h('span', { id: 'quiz-label' }, `Quiz · Question ${item.number} of ${quiz.items.length}`),
      dots(quiz.items),
    ),
    h('h2', { class: 'quiz-item', tabindex: -1 }, item.question),
    h(
      'p',
      { class: 'hint' },
      'The quiz checks what you took from the design; nothing here marks lines.',
      checked ? null : ' Its proof shows once you check your answer.',
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
 * A dot for each item: green when its pick was correct, red when wrong, an outline when not
 * answered yet. The colours say it at a glance; the dots' name says it in words.
 * @param {QuizItemView[]} items
 */
function dots(items) {
  const words = items.map((item) => `question ${item.number} ${OUTCOMES[outcomeOf(item)].dotWords}`).join(', ');
  const name = words.charAt(0).toUpperCase() + words.slice(1);
  return h(
    'span',
    { class: 'quiz-dots', role: 'img', 'aria-label': name, title: name },
    items.map((item) => h('span', { class: `dot ${OUTCOMES[outcomeOf(item)].dot}` })),
  );
}

/**
 * The panel before Check: the answers as choice cards, Check, which waits for a pick, and Skip
 * the quiz while there is a quiz to skip.
 * @param {QuizItemView} item
 * @param {string} conclusion
 * @param {boolean} asks whether the quiz asks for more answers, which Skip the quiz declines
 */
function pickPanel(item, conclusion, asks) {
  const answers = item.answers.map((answer) => ({
    id: String(answer.index),
    text: answer.text,
    recommendation: null,
    checked: false,
  }));
  return panel(
    item,
    h(
      'form',
      { class: 'quiz-pick', 'data-method': 'quiz', 'data-requires': 'answer' },
      h('input', { type: 'hidden', name: 'conclusion', value: conclusion }),
      h('input', { type: 'hidden', name: 'item', value: item.number - 1 }),
      choiceCards(answers, { name: 'answer', legend: 'Answers' }),
      h('button', { class: 'button primary block', type: 'submit' }, 'Check'),
    ),
    asks ? skipForm(conclusion) : null,
  );
}

/**
 * The panel after Check: the verdict, which the page brings into view, the answers with their
 * marks, then the next step.
 * @param {QuizItemView} item
 * @param {string} conclusion
 * @param {boolean} asks whether the quiz asks for more answers
 * @param {() => void} next
 */
function answeredPanel(item, conclusion, asks, next) {
  return panel(
    item,
    verdict(item, { role: 'status', tabindex: -1, 'data-shows': 'quiz' }),
    answerList(item, 'quiz-answers-title'),
    h(
      'button',
      { class: 'button primary block', type: 'button', onclick: next },
      asks ? 'Next question' : 'Show the conclusion',
      h('span', { 'aria-hidden': 'true' }, '→'),
    ),
    asks ? skipForm(conclusion) : null,
  );
}

/** The panel of the item, holding `children`.
 * @param {QuizItemView} item
 * @param {...import('./dom.js').Children} children */
function panel(item, ...children) {
  return h('section', { class: 'quiz-panel panel', 'aria-label': `Your answer to quiz question ${item.number}` }, children);
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
  return h('p', { class: `verdict ${outcome.tone}`, ...props }, h('span', {}, h('strong', {}, words), ` ${item.why}`));
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
    h('span', { class: 'choice-text' }, answer.text),
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
 * The results of the quiz, beside the conclusion: the score as its heading, then, behind a fold,
 * each item with its verdict, its answers with their marks, and its proof.
 * @param {QuizView} quiz
 */
export function quizResults(quiz) {
  const unanswered = quiz.items.length - quiz.picked;
  const missed = unanswered > 0 ? `, ${unanswered} ${quiz.skipped ? 'skipped' : 'not answered'}` : '';
  const { element } = disclosure(
    'The answers, question by question',
    quiz.items.map((item) => {
      const id = `quiz-result-${item.number}`;
      return h(
        'section',
        { class: 'quiz-result', 'aria-labelledby': id },
        h('h4', { class: 'eyebrow', id }, `Question ${item.number}`),
        h('p', { class: 'quiz-result-item' }, item.question),
        verdict(item, {}, quiz.skipped ? 'Skipped.' : undefined),
        answerList(item, `${id}-answers`),
        item.proof.map((each, index) => citation(each, `${id}-proof-${index + 1}`)),
      );
    }),
  );
  return h(
    'section',
    { class: 'quiz-results', id: 'quiz-results', 'aria-labelledby': 'quiz-results-title' },
    h(
      'h3',
      { class: 'eyebrow', id: 'quiz-results-title' },
      `Quiz · ${quiz.correct_picks} of ${quiz.items.length} correct${missed}`,
    ),
    element,
  );
}

/** Opens the results of the quiz, from a link to them. */
export function openQuizResults() {
  const fold = document.querySelector('#quiz-results > .disclosure');
  if (fold) openDisclosure(fold);
}
