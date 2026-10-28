// Past rounds: the repository's saved rounds, which the page on this machine lists in its ⋯ menu
// and opens read only (route.js names one in the address). The line above the screens says that
// the page shows a past round, of which change and when, says when the round the page follows
// waits for the reviewer meanwhile, and leads back to it. The menu's list groups the rounds by
// change, the change with the newest round first, each round named by its date.

/** @import { PastChange, PastView, ReviewName } from "./types.ts" */

import { h } from './dom.js';
import { revisionCode } from './revision.js';
import { pastAddress, STAGE } from './route.js';

const DATE = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' });

/** When a round was saved, as the page shows it.
 * @param {number} ms milliseconds since the epoch */
export function savedAt(ms) {
  return DATE.format(new Date(ms));
}

/** The link back to the round the page follows, of the class `kind`: the banner's, or the
 * menu's entry.
 * @param {string} kind */
export function backLink(kind) {
  return h('a', { class: kind, href: STAGE, 'data-leave-past': '' }, 'Back to the current round');
}

/**
 * The line that says the page shows a past round, read only.
 * @param {PastView} past
 * @param {ReviewName | null} review the change the past round reviewed
 */
export function pastBanner(past, review) {
  return h(
    'section',
    { class: 'past-banner', 'aria-label': 'Past round' },
    h(
      'p',
      { class: 'past-what' },
      h('span', { class: 'eyebrow' }, 'Past round · read only'),
      ' ',
      h('span', { class: 'past-title' }, review?.title),
      ' ',
      h('time', { class: 'past-date', datetime: new Date(past.saved_at_ms).toISOString() }, savedAt(past.saved_at_ms)),
    ),
    h(
      'p',
      { class: 'past-current' },
      past.current_waits ? h('span', { class: 'past-waits' }, 'The current round waits for you.') : null,
      ' ',
      backLink('past-back'),
    ),
  );
}

/**
 * The menu's list of past rounds, grouped by change.
 * @param {PastChange[]} changes
 * @param {string | null} shown the past round the page shows, if any
 */
export function pastList(changes, shown) {
  return h(
    'nav',
    { class: 'past-list', 'aria-label': 'Past rounds' },
    h('p', { class: 'eyebrow' }, 'Past rounds'),
    changes.map((change) =>
      h(
        'section',
        { class: 'past-change', 'aria-label': change.title },
        h('p', { class: 'past-change-title' }, change.title, ' ', revisionCode(change.revision)),
        h(
          'ul',
          {},
          change.rounds.map((round) =>
            h(
              'li',
              {},
              h(
                'a',
                {
                  class: 'menu-item past-round',
                  href: pastAddress(round.id),
                  'aria-current': round.id === shown ? 'page' : null,
                },
                h('time', { datetime: new Date(round.saved_at_ms).toISOString() }, savedAt(round.saved_at_ms)),
              ),
            ),
          ),
        ),
      ),
    ),
  );
}
