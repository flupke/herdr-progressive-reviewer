// The previous turn, in one quiet block above the stage it led to: what the reviewer answered,
// beside what the agent recorded of it and replied, then the follow-ups the agent noted and
// Cancel this answer, behind its confirmation. Cancel answer takes the answer back, as in the
// reviewer's Explore tab: the round goes back to its question, and the review marks the answer
// led to are given back.

/** @import { LatestAnswer, ResponseView } from "./types.ts" */

import { disclosure } from './disclosure.js';
import { h, markdown } from './dom.js';

/**
 * @typedef {object} Turn
 * @property {LatestAnswer | null} answer the reviewer's latest answer, when the page offers to
 *   cancel it
 * @property {ResponseView | null} response what the agent's turn said back to it
 * @property {number | null} number the number of the question the answer answered, when known
 */

/**
 * The block, or `null` when the turn has nothing to show.
 * @param {Turn} turn
 */
export function turnStrip({ answer, response, number }) {
  if (!answer && !response) return null;
  const followUps = response?.interpretations.flatMap((interpretation) => interpretation.follow_ups) ?? [];
  return h(
    'div',
    { class: 'turn' },
    answer ? answered(answer, number) : null,
    response ? recorded(response) : null,
    followUps.length > 0 || answer
      ? h(
          'div',
          { class: 'turn-foot' },
          followUps.length > 0 ? h('p', { class: 'turn-follow-ups' }, ['Follow-ups', ...followUps].join(' · ')) : null,
          answer ? cancelAnswer(answer) : null,
        )
      : null,
  );
}

/**
 * What the reviewer answered: the choice, then the comment.
 * @param {LatestAnswer} answer
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
      answer.choice ? h('p', { class: 'turn-choice' }, answer.choice) : null,
      answer.comment ? h('p', { class: 'turn-comment' }, `“${answer.comment}”`) : null,
      !answer.choice && !answer.comment ? h('p', { class: 'turn-comment' }, 'No choice, no comment') : null,
    ),
  );
}

/**
 * What the agent recorded of the answer, its recap, then its reply.
 * @param {ResponseView} response
 */
function recorded(response) {
  const recaps = response.interpretations.flatMap((interpretation) =>
    interpretation.recap_html ? [interpretation.recap_html] : [],
  );
  const title = recaps.length > 0 ? 'The agent recorded' : 'The agent replied';
  return h(
    'section',
    { class: 'turn-record', 'aria-labelledby': 'turn-record-title' },
    h('p', { class: 'eyebrow', id: 'turn-record-title' }, title),
    h(
      'div',
      { class: 'turn-text' },
      recaps.map((recap) => markdown(recap, 'turn-recap')),
      response.reply_html ? markdown(response.reply_html, 'turn-reply') : null,
    ),
  );
}

/** @param {LatestAnswer} answer */
function cancelAnswer(answer) {
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
