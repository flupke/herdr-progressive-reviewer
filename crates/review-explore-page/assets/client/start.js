// The start cover, when no round is running: the review the page belongs to as the headline,
// where it is and how large its change is, then whose change it is, then Start and Start with
// Challenger, each with one line that says what it does. For someone else's change, the
// reviewer may give their name, which the agent reads.
// The status cards above it say why the latest start failed, or that nothing is left to review,
// in place of the eyebrow; then both buttons are inactive, described by that card.

/** @import { MarkTally, StartView, ReviewName } from "./types.ts" */

import { h } from './dom.js';
import { keep, keepDraft, kept } from './drafts.js';
import { changeSize, fileCount } from './change-size.js';
import { revisionCode, revisionText } from './revision.js';

/**
 * @param {StartView} start
 * @param {ReviewName | null} review
 * @param {MarkTally | null} tally the review marks of the change, which give its size
 */
export function startCover(start, review, tally) {
  // Whose change it is, mine unless the reviewer says otherwise, and their name for someone
  // else's.
  // Both are kept for the tab, as drafts are, so that a rebuild of the cover keeps them.
  const whose = () => {
    const others = kept('start:whose') === 'others';
    const name = keepDraft(
      h('input', { type: 'text', name: 'reviewer', autocomplete: 'name', maxlength: 80 }),
      'start:reviewer',
      '',
    );
    const named = h('label', { class: 'start-name', hidden: !others }, 'Your name, for the agent · optional', name);
    const option = (/** @type {string} */ value, /** @type {string} */ label) =>
      h(
        'label',
        { class: 'start-whose-option' },
        h('input', {
          type: 'radio',
          name: 'whose',
          value,
          'data-focus-key': `start-whose-${value}`,
          checked: (value === 'others') === others,
          onchange: () => {
            keep('start:whose', value);
            named.hidden = value !== 'others';
          },
        }),
        label,
      );
    return h(
      'fieldset',
      { class: 'start-whose' },
      h('legend', { class: 'eyebrow' }, 'Whose change'),
      option('mine', 'Mine'),
      option('others', "Someone else's: learn it, then review it for its author"),
      named,
    );
  };
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
    review ? h('h1', {}, review.title || revisionText(review.revision)) : null,
    review
      ? h(
          'p',
          { class: 'meta' },
          revisionCode(review.revision),
          ' in ',
          h('code', {}, review.repository),
          tally && tally.files.length > 0 ? [' · ', changeSize(tally), ` in ${fileCount(tally.files.length)}`] : null,
        )
      : null,
    h(
      'form',
      { class: 'start', 'data-method': 'start' },
      h('input', { type: 'hidden', name: 'start', value: start.start }),
      whose(),
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
