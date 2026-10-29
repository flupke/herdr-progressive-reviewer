// Past rounds: the repository's saved rounds, which the page on this machine lists in the column
// at the window's left (left-column.js) and opens read only (route.js names one in the address).
// The ⋯ menu opens the list, as does the address the pane opens; it stays open beside the page
// while the reviewer reads one round after another, and closes as the page goes to another screen
// or round only where it lies over the page. It groups the rounds by change, the change with the
// newest round first, each change named by its title and its revision, each round by when it was
// saved: the day once for the rounds of the same day, the time of each in a column of its own.
// The line above the screens says that the page shows a past round, of which change and when,
// says when the round the page follows waits for the reviewer meanwhile, and leads back to it.

/** @import { PageView, PastChange, PastRoundEntry, PastView, ReviewName } from "./types.ts" */

import { BESIDE } from './desk.js';
import { h, keyOf, Region } from './dom.js';
import { LeftColumn } from './left-column.js';
import { revisionCode } from './revision.js';
import { PAST_ROUNDS, pastAddress, STAGE } from './route.js';

const DATE = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' });
const DAY = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium' });
const TIME = new Intl.DateTimeFormat(undefined, { timeStyle: 'short' });

/** The event that asks the list to open (`requestPastList`). */
const OPEN = 'explore:past-rounds';

/**
 * Opens the list of past rounds.
 * @param {() => Element | null} [returnTo] what gets the focus back once the list closes, found
 *   then: what opened it may have been drawn again meanwhile
 */
export function requestPastList(returnTo) {
  document.dispatchEvent(new CustomEvent(OPEN, { detail: { returnTo } }));
}

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

/** The list of past rounds, in the column at the window's left. */
export class PastList {
  /** @param {() => void} redraw draws the page again, for the list the address asks for */
  constructor(redraw) {
    this.column = new LeftColumn({
      tag: 'nav',
      kind: 'past-list',
      label: 'Past rounds',
      title: 'Past rounds',
      closeLabel: 'Close the past rounds',
      closed: (refocus) => this.closed(refocus),
    });
    // Focusable, for the list to take the focus when it marks no round: a round focused instead
    // would look like the one the page shows.
    this.body = h('div', { class: 'past-body', tabindex: '-1' });
    this.column.element.append(this.body);
    this.rounds = new Region(this.body, 'rounds');
    /** What gets the focus back as the list closes: what opened it, or else what had the focus.
     * @type {(() => Element | null) | null} */
    this.opener = null;
    /** Whether the address asked for the list, which opens with the next view that has one. */
    this.asked = location.hash === PAST_ROUNDS;
    // The address of the list opens it. Where the list lies over the page, it closes as the page
    // goes to another screen or round (from the list, the banner's way back, the rail), so that
    // the page shows whole.
    addEventListener('hashchange', () => {
      if (location.hash === PAST_ROUNDS) {
        this.asked = true;
        redraw();
      } else if (!BESIDE.matches) this.column.close(false);
    });
    document.addEventListener(OPEN, (event) => {
      const detail = /** @type {CustomEvent<{ returnTo?: () => Element | null }>} */ (event).detail;
      this.show(detail.returnTo);
    });
    // The round the page shows already changes no address: a tap on it closes the list too.
    this.column.element.addEventListener('click', (event) => {
      if (BESIDE.matches || !(event.target instanceof Element)) return;
      if (event.target.closest('.past-round[aria-current="page"]')) this.column.close(false);
    });
  }

  /** @param {PageView} view */
  update(view) {
    const changes = view.past_rounds;
    if (changes === null) {
      // The page on the network lists none (ADR 0003, ADR 0004).
      this.column.close(false);
      this.rounds.clear();
      if (this.asked) this.leaveAsked();
      return;
    }
    const count = changes.reduce((sum, change) => sum + change.rounds.length, 0);
    this.column.meta.textContent = count === 0 ? '' : `${count} ${count === 1 ? 'round' : 'rounds'}`;
    const shown = view.past?.round ?? null;
    this.rounds.show(keyOf([changes, shown]), () => pastChanges(changes, shown));
    if (this.asked) {
      this.leaveAsked();
      this.show();
    }
  }

  /** Leaves the address the list was asked at for the stage, so that the browser's Back and a
   * reload do not open it again. */
  leaveAsked() {
    this.asked = false;
    history.replaceState(history.state, '', STAGE);
  }

  /** Opens the list, and gives the focus to the round the page shows, in view with its change's
   * title, or else to the list, scrolled to its top.
   * @param {() => Element | null} [returnTo] what gets the focus back once the list closes */
  show(returnTo) {
    // Read first: opening the list closes the chat, which draws the page again.
    const focused = document.activeElement;
    if (this.column.show()) this.opener = returnTo ?? (() => focused);
    const element = this.column.element;
    const marked = element.querySelector('.past-round[aria-current="page"]');
    if (marked instanceof HTMLElement) {
      marked.focus({ preventScroll: true });
      marked.closest('.past-change')?.scrollIntoView({ block: 'nearest' });
      // A change taller than the list shows its top: the round then comes into view below it.
      marked.scrollIntoView({ block: 'nearest' });
      return;
    }
    this.body.focus({ preventScroll: true });
    this.body.scrollTo(0, 0);
  }

  /** @param {boolean} refocus whether to give the focus back to what had it when the list opened */
  closed(refocus) {
    const opener = this.opener?.() ?? null;
    if (refocus && opener instanceof HTMLElement && opener.isConnected) opener.focus();
    this.opener = null;
  }
}

/**
 * The past rounds, by change, or a line that says there is none yet, for the pane's way to the
 * list.
 * @param {PastChange[]} changes
 * @param {string | null} shown the past round the page shows, if any
 */
function pastChanges(changes, shown) {
  if (changes.length === 0) return h('p', { class: 'past-none' }, 'No past round yet.');
  return changes.map((change) =>
    h(
      'section',
      { class: 'past-change', 'aria-label': change.title },
      h(
        'div',
        { class: 'past-change-head' },
        h('h3', { class: 'past-change-title', title: change.title }, change.title),
        revisionCode(change.revision),
      ),
      h(
        'ul',
        { class: 'past-rounds' },
        change.rounds.map((round, index) => pastRound(round, shown, change.rounds[index - 1])),
      ),
    ),
  );
}

/**
 * A round of a change, as a link that opens it: the day it was saved, unless the round above was
 * saved the same day, and the time.
 * @param {PastRoundEntry} round
 * @param {string | null} shown the past round the page shows, if any
 * @param {PastRoundEntry | undefined} above the change's round listed above it
 */
function pastRound(round, shown, above) {
  const day = DAY.format(new Date(round.saved_at_ms));
  const sameDay = above !== undefined && DAY.format(new Date(above.saved_at_ms)) === day;
  return h(
    'li',
    {},
    h(
      'a',
      {
        class: 'past-round',
        href: pastAddress(round.id),
        'aria-current': round.id === shown ? 'page' : null,
        'aria-label': savedAt(round.saved_at_ms),
        'data-focus-key': `past:${round.id}`,
      },
      h('span', { class: sameDay ? 'past-day same' : 'past-day', 'aria-hidden': 'true' }, day),
      h(
        'time',
        { class: 'past-time', datetime: new Date(round.saved_at_ms).toISOString(), 'aria-hidden': 'true' },
        TIME.format(new Date(round.saved_at_ms)),
      ),
    ),
  );
}
