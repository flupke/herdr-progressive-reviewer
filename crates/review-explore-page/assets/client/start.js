// The start cover, when no round is running: the review the page belongs to as the headline,
// where it is, then Start and Start with Challenger, each with one line that says what it does.
// The status cards above it say why the latest start failed, or that nothing is left to review,
// in place of the eyebrow; then both buttons are inactive, described by that card.

/** @import { StartView, ReviewName } from "./types.ts" */

import { h } from './dom.js';

/**
 * @param {StartView} start
 * @param {ReviewName | null} review
 */
export function startCover(start, review) {
  const choice = (/** @type {string} */ label, /** @type {string} */ about, /** @type {string} */ id, challenger = false) =>
    h(
      'div',
      { class: 'start-choice' },
      h(
        'button',
        {
          class: challenger ? 'button secondary block' : 'button primary block',
          type: 'submit',
          name: challenger ? 'challenger' : null,
          value: challenger ? 'true' : null,
          'aria-describedby': start.block ?? id,
          disabled: start.block !== null,
          'data-blocked': start.block !== null ? 'true' : null,
        },
        label,
      ),
      h('p', { class: 'hint', id }, about),
    );
  return h(
    'section',
    { class: 'start-cover' },
    start.idle ? h('p', { class: 'eyebrow', role: 'status' }, 'No round is running') : null,
    review ? h('h1', {}, review.title || review.revision) : null,
    review
      ? h('p', { class: 'meta' }, h('code', {}, review.revision), ' in ', h('code', {}, review.repository))
      : null,
    h(
      'form',
      { class: 'start', 'data-method': 'start' },
      h('input', { type: 'hidden', name: 'start', value: start.start }),
      choice('Start', 'The agent explains the design, then asks one question at a time.', 'start-about'),
      choice(
        'Start with Challenger',
        'A second agent with fresh context proposes questions too. Turns take longer.',
        'challenger-about',
        true,
      ),
    ),
  );
}
