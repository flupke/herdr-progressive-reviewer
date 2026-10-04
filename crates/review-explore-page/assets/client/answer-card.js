// The reviewer's answer as a card, and what it marked: in the panel of an earlier question, and
// in the panel of the answer the agent's turn carries (docs/design/explore-page/README.md,
// "3. Waiting"). Styled in answer-card.css.

/** @import { KeptAnswer, MarkPhrase } from "./types.ts" */

import { decisionTag } from './chips.js';
import { codeSpans, h } from './dom.js';

/** What an answer with neither a choice nor a comment shows in their place: in its card, and in
 * the previous turn (turn.js). */
export const NOTHING_ANSWERED = 'No choice, no comment';

/** The answer the reviewer sent or kept: the choice, unless the choices show beside it
 * (`withChoice`), the comment, and how the choice relates to the first pick and to the agent's
 * recommendation; `null` when nothing is left to show.
 * @param {KeptAnswer} answer
 * @param {{ withChoice?: boolean }} [options] */
export function answerCard(answer, { withChoice = true } = {}) {
  if (!withChoice && answer.choice !== null && !answer.comment && answer.tags.length === 0) return null;
  return h(
    'div',
    { class: 'answer-card' },
    withChoice && answer.choice !== null ? h('p', { class: 'answer-choice' }, codeSpans(answer.choice)) : null,
    answer.comment ? h('p', { class: 'answer-comment' }, `“${answer.comment}”`) : null,
    answer.choice === null && !answer.comment ? h('p', { class: 'answer-comment' }, NOTHING_ANSWERED) : null,
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
