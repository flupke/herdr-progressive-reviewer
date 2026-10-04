// The reviewer's latest answer, with Cancel answer, as in the reviewer's Explore tab: the round
// goes back to its question, and the review marks the answer led to are given back.

/** @import { LatestAnswer } from "./types.ts" */

import { h } from './dom.js';

/** @param {LatestAnswer} answer */
export function lastAnswer(answer) {
  return h(
    'section',
    { class: 'last-answer', 'aria-labelledby': 'last-answer-title' },
    h('h2', { id: 'last-answer-title' }, 'Your last answer'),
    answer.choice ? h('p', { class: 'choice' }, answer.choice) : null,
    answer.comment ? h('p', { class: 'answer-comment' }, answer.comment) : null,
    h(
      'form',
      { class: 'cancel-answer', 'data-method': 'cancel-answer' },
      h('input', { type: 'hidden', name: 'answer', value: answer.id }),
      h(
        'p',
        { class: 'hint' },
        "Cancel answer takes this answer back, with the agent's turn after it and the review marks it led to: its question waits for your answer again.",
      ),
      h('button', { class: 'button quiet', type: 'submit' }, 'Cancel answer'),
    ),
  );
}
