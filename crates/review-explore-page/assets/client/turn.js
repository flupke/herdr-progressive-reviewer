// The previous turn, in one quiet block above the stage it led to: what the reviewer answered,
// beside what the agent recorded of it and replied, then, when run-ahead watched the question,
// whether it prepared the turn or why not, the follow-ups the agent noted and Cancel this
// answer, behind its confirmation. Cancel answer takes the answer back, as in the reviewer's
// Explore tab: the round goes back to its question, and the review marks the answer led to are
// given back.

/** @import { KeptAnswer, LatestAnswer, ResponseView } from "./types.ts" */

import { NOTHING_ANSWERED } from './answer-card.js';
import { disclosure } from './disclosure.js';
import { codeSpans, h, markdown } from './dom.js';

/**
 * @typedef {object} Turn
 * @property {LatestAnswer | null} answer the reviewer's latest answer, when the page offers to
 *   cancel it
 * @property {KeptAnswer | null} [kept] the reviewer's latest answer as the round kept it, when it
 *   can no longer be cancelled
 * @property {ResponseView | null} response what the agent's turn said back to it
 * @property {number | null} number the number of the question the answer answered, when known
 */

/**
 * The block, or `null` when the turn has nothing to show.
 * @param {Turn} turn
 */
export function turnStrip({ answer, kept = null, response, number }) {
  if (!answer && !response) return null;
  const followUps = response ? followUpsLine(response, 'turn-follow-ups') : null;
  // One quiet line: a fork took the turn while the reviewer thought about the answer, or why the
  // agent took it itself.
  const path = response?.path_line ? h('p', { class: 'turn-path' }, response.path_line) : null;
  const shown = answer ?? kept;
  return h(
    'div',
    { class: 'turn' },
    shown ? answered(shown, number) : null,
    response ? recorded(response) : null,
    path || followUps || answer
      ? h(
          'div',
          { class: 'turn-foot' },
          path,
          followUps,
          answer ? cancelAnswer(answer) : null,
        )
      : null,
  );
}

/**
 * What the reviewer answered: the choice, then the comment.
 * @param {LatestAnswer | KeptAnswer} answer
 * @param {number | null} number
 */
function answered(answer, number) {
  const title = number === null ? 'You answered' : `You answered Q${number}`;
  return h(
    'section',
    { class: 'turn-answer', 'aria-labelledby': 'turn-answer-title' },
    h('p', { class: 'eyebrow', id: 'turn-answer-title' }, title),
    h(
      'div',
      { class: 'turn-text' },
      answer.choice ? h('p', { class: 'turn-choice' }, codeSpans(answer.choice)) : null,
      answer.comment ? h('p', { class: 'turn-comment' }, `“${answer.comment}”`) : null,
      !answer.choice && !answer.comment ? h('p', { class: 'turn-comment' }, NOTHING_ANSWERED) : null,
    ),
  );
}

/**
 * "Follow-ups · …", the follow-ups the agent noted of the answer, or `null` when it noted none: in
 * the previous turn, and on the screen of an earlier question.
 * @param {ResponseView} response
 * @param {string} className
 */
export function followUpsLine(response, className) {
  const followUps = response.interpretations.flatMap((interpretation) => interpretation.follow_ups);
  return followUps.length > 0
    ? h('p', { class: className }, 'Follow-ups', followUps.map((followUp) => [' · ', codeSpans(followUp)]))
    : null;
}

/**
 * What the agent recorded of the answer, its recap, then its reply: in the previous turn, and on
 * the screen of an earlier question.
 * @param {ResponseView} response
 * @param {string} [id] the ID of its eyebrow, which names it
 */
export function recorded(response, id = 'turn-record-title') {
  const recaps = response.interpretations.flatMap((interpretation) =>
    interpretation.recap_html ? [interpretation.recap_html] : [],
  );
  const title = recaps.length > 0 ? 'The agent recorded' : 'The agent replied';
  return h(
    'section',
    { class: 'turn-record', 'aria-labelledby': id },
    h('p', { class: 'eyebrow', id }, title),
    h(
      'div',
      { class: 'turn-text' },
      recaps.map((recap) => markdown(recap, 'turn-recap')),
      response.reply_html ? markdown(response.reply_html, 'turn-reply') : null,
    ),
  );
}

/**
 * Cancel this answer, behind its confirmation: in the previous turn, and in the panel of the
 * answer the agent's turn carries.
 * @param {LatestAnswer} answer
 */
export function cancelAnswer(answer) {
  return disclosure(
    'Cancel this answer…',
    h(
      'form',
      { class: 'cancel-answer', 'data-method': 'cancel-answer' },
      h('input', { type: 'hidden', name: 'answer', value: answer.id }),
      h(
        'p',
        { class: 'hint' },
        "Cancel answer takes this answer back, with the agent's turn after it and the review marks it led to: its question waits for your answer again.",
      ),
      h('button', { class: 'button secondary', type: 'submit' }, 'Confirm: cancel my answer'),
    ),
    { button: 'button quiet' },
  ).element;
}
