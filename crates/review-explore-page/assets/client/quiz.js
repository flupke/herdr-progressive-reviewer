// The conclusion's quiz, before the conclusion, one item at a time: the item the reviewer
// answers next, or the one just answered, with whether the pick is correct, why, and the lines
// that prove it. The reviewer may skip the rest. Once every item is answered or skipped, the
// results fold beside the conclusion (quizResults).
//
// Which item shows is the page's own state: after a pick, the item just answered shows until
// the reviewer moves on with "Next question".

/** @import { QuizView, QuizItemView } from "./types.ts" */

import { citation } from './citations.js';
import { h } from './dom.js';

/**
 * The quiz in place of the conclusion, showing the item `shown`, from 0.
 * @param {QuizView} quiz
 * @param {string} conclusion the request of the turn that posted the conclusion
 * @param {number} shown
 * @param {() => void} next moves on from an answered item
 */
export function quizSection(quiz, conclusion, shown, next) {
  const item = /** @type {QuizItemView} */ (quiz.items[shown]);
  const asks = quiz.next !== null;
  const answered = item.picked_correct !== null;
  return h(
    'section',
    { class: 'quiz', 'aria-labelledby': 'quiz-title' },
    h('h2', { id: 'quiz-title' }, 'Quiz'),
    h('p', { class: 'hint' }, 'Before the conclusion, check that you could explain this change at a whiteboard.'),
    h('h3', { id: 'quiz-item-title' }, `Question ${item.number} of ${quiz.items.length}`),
    answered
      ? [
          h('p', { class: 'text' }, item.question),
          answeredItem(item, 'quiz-item', true, false),
          h(
            'p',
            {},
            h(
              'a',
              {
                class: 'next',
                href: '/',
                onclick: (/** @type {Event} */ event) => {
                  event.preventDefault();
                  next();
                },
              },
              asks ? 'Next question' : 'Show the conclusion',
            ),
          ),
        ]
      : h(
          'form',
          { class: 'quiz-pick', 'data-method': 'quiz' },
          h('input', { type: 'hidden', name: 'conclusion', value: conclusion }),
          h('input', { type: 'hidden', name: 'item', value: item.number - 1 }),
          h(
            'fieldset',
            { class: 'choices' },
            h('legend', { class: 'text' }, item.question),
            item.answers.map((answer) =>
              h(
                'label',
                { class: 'choice' },
                h('input', { type: 'radio', name: 'answer', value: answer.index, required: true }),
                ` ${answer.text}`,
              ),
            ),
          ),
          h('button', { class: 'button primary', type: 'submit' }, 'Check'),
        ),
    asks
      ? h(
          'form',
          { class: 'quiz-skip', 'data-method': 'quiz-skip' },
          h('input', { type: 'hidden', name: 'conclusion', value: conclusion }),
          h('button', { class: 'button quiet', type: 'submit' }, 'Skip the quiz'),
        )
      : null,
  );
}

/**
 * The results of the quiz, folded beside the conclusion.
 * @param {QuizView} quiz
 */
export function quizResults(quiz) {
  const unanswered = quiz.items.length - quiz.picked;
  const missed = unanswered > 0 ? `, ${unanswered} ${quiz.skipped ? 'skipped' : 'not answered'}` : '';
  return h(
    'section',
    { class: 'quiz-results', 'aria-labelledby': 'quiz-results-title' },
    h(
      'details',
      {},
      h(
        'summary',
        {},
        h('h3', { id: 'quiz-results-title' }, `Quiz: ${quiz.correct_picks} of ${quiz.items.length} correct${missed}`),
      ),
      quiz.items.map((item) => [
        h('h4', {}, `Question ${item.number}`),
        h('p', { class: 'text' }, item.question),
        answeredItem(item, `quiz-result-${item.number}`, false, quiz.skipped),
      ]),
    ),
  );
}

/** Opens the results of the quiz, from a link to them. */
export function openQuizResults() {
  const fold = document.querySelector('.quiz-results > details');
  if (fold instanceof HTMLDetailsElement) fold.open = true;
}

/**
 * What became of one item: its options, with the reviewer's pick and the correct one, whether the
 * pick was correct, why, and the lines that prove it. `id` names the item's headings; `status`
 * makes the verdict the page's status, on the item the page shows alone; `skipped` tells that
 * the reviewer skipped an item without a pick.
 * @param {QuizItemView} item
 * @param {string} id
 * @param {boolean} status
 * @param {boolean} skipped
 */
function answeredItem(item, id, status, skipped) {
  const verdict =
    item.picked_correct === null
      ? `${skipped ? 'Skipped.' : 'Not answered.'} The correct answer: ${item.correct_answer}`
      : item.picked_correct
        ? 'Correct.'
        : `Not quite. The correct answer: ${item.correct_answer}`;
  const tone = item.picked_correct === null ? ' skipped' : item.picked_correct ? ' good' : '';
  return [
    h(
      'ul',
      { class: 'quiz-answers' },
      item.answers.map((answer) => {
        const marks = [answer.correct ? 'correct' : null, answer.picked ? 'picked' : null].filter(Boolean);
        return h(
          'li',
          { class: marks.length > 0 ? marks.join(' ') : null },
          // One text before each tag, as the browser would parse it from markup.
          answer.picked || answer.correct ? `${answer.text} ` : answer.text,
          answer.picked ? h('span', { class: 'tag' }, 'your pick') : null,
          answer.picked && answer.correct ? ' ' : null,
          answer.correct ? h('span', { class: 'tag' }, 'correct answer') : null,
        );
      }),
    ),
    h('p', { class: `verdict${tone}`, role: status ? 'status' : null }, verdict),
    h('p', { class: 'why' }, item.why),
    h(
      'section',
      { class: 'proof', 'aria-labelledby': `${id}-proof` },
      h('h4', { id: `${id}-proof` }, 'Proof'),
      item.proof.map((proof, index) => citation(proof, `${id}-proof-${index + 1}`)),
    ),
  ];
}
