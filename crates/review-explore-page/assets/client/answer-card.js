// The reviewer's answer as a card, and what it marked: in the panel of an earlier question, and
// in the panel of the answer the agent's turn carries (docs/design/explore-page/README.md,
// "3. Waiting"). Styled in answer-card.css.

/** @import { KeptAnswer, MarkPhrase } from "./types.ts" */

import { decisionTag } from './chips.js';
import { h } from './dom.js';

/** The answer the reviewer sent or kept: the choice, the comment, and how the choice relates to
 * the first pick and to the agent's recommendation.
 * @param {KeptAnswer} answer */
export function answerCard(answer) {
  return h(
    'div',
    { class: 'answer-card' },
    answer.choice !== null ? h('p', { class: 'answer-choice' }, answer.choice) : null,
    answer.comment ? h('p', { class: 'answer-comment' }, `“${answer.comment}”`) : null,
    answer.choice === null && !answer.comment ? h('p', { class: 'answer-comment' }, 'No choice, no comment') : null,
    answer.tags.length > 0 ? h('p', { class: 'answer-tags' }, answer.tags.map(decisionTag)) : null,
  );
}

/** "✓ Marked 12 lines reviewed · 3 lines not relevant". @param {MarkPhrase} marks */
export function markedLine(marks) {
  return h(
    'p',
    { class: 'answer-marked' },
    h('span', { class: 'check', 'aria-hidden': 'true' }, '✓'),
    ` ${marks.verb} ${marks.parts.join(' · ')}`,
  );
}
