// Reset, as in the reviewer's Explore tab, behind a confirmation: it closes the round for good.

import { h } from './dom.js';

/** @param {string} round the round Reset closes */
export function resetFold(round) {
  return h(
    'details',
    { class: 'reset' },
    h('summary', {}, 'Reset'),
    h(
      'form',
      { class: 'reset', 'data-method': 'reset' },
      h('input', { type: 'hidden', name: 'round', value: round }),
      h(
        'p',
        { class: 'hint' },
        'Reset closes this round for good: it is no longer shown, its agent can no longer post to it, and the page offers to start a new round. Its records stay saved.',
      ),
      h('button', { class: 'button danger', type: 'submit' }, 'Confirm reset'),
    ),
  );
}
