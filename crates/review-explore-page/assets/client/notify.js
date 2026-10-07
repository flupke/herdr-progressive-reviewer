// A browser notification when the agent finishes something while the reviewer looks elsewhere:
// a turn ends (the next question, the conclusion, or a retry the round needs), or the agent
// replies in the chat. The reviewer turns it on in the ⋯ menu (masthead.js), for this browser
// only, as the browser's own permission is; it is off until then.
//
// The page notifies only of a change it saw happen: the first view after the page loads sets
// where the round stands. It stays quiet while the page has the focus, since the reviewer sees
// the change there.

/** @import { PageView, TabTitle } from "./types.ts" */

import { revisionText } from './revision.js';

/** The browser's record of the reviewer's choice. */
const KEY = 'explore-notify';

/** Whether the reviewer can turn notifications on, and if not, why.
 * @typedef {'on' | 'off' | 'blocked' | 'unavailable'} NotifyState */

export class Notifier {
  /** @param {() => void} changed called when the state the menu shows changes */
  constructor(changed) {
    this.changed = changed;
    this.wanted = read() === 'on';
    /** The tab's title in the latest view, or null before the first.
     * @type {{ title: TabTitle | null } | null} */
    this.last = null;
    /** The agent's replies in the chat that the page has seen, by their identity. They stay while
     * a stage shows no chat, so the chat's return brings no old reply back as new.
     * @type {Set<string>} */
    this.replies = new Set();
  }

  /** What the menu shows. @returns {NotifyState} */
  state() {
    // Browsers offer notifications to secure pages only: not to the page's plain-http network
    // address.
    if (!('Notification' in window) || !window.isSecureContext) return 'unavailable';
    if (Notification.permission === 'denied') return 'blocked';
    return this.wanted && Notification.permission === 'granted' ? 'on' : 'off';
  }

  /** Turns notifications on or off; on asks the browser's permission the first time. */
  async toggle() {
    const state = this.state();
    if (state === 'unavailable' || state === 'blocked') return;
    this.wanted = state === 'off';
    if (this.wanted && Notification.permission === 'default') {
      // A reviewer who dismisses or refuses the browser's question leaves them off: granting it
      // later in the browser's settings does not turn them on behind the menu's back.
      this.wanted = (await Notification.requestPermission()) === 'granted';
    }
    write(this.wanted ? 'on' : 'off');
    this.changed();
  }

  /** @param {PageView} view */
  update(view) {
    const last = this.last;
    this.last = { title: view.title };
    // The replies themselves, not the unread ones: the chat counts none unread while it shows
    // them, also to a reviewer who works in another window.
    let added = 0;
    for (const message of view.conversation?.messages ?? []) {
      if (message.author !== 'agent' || this.replies.has(message.id)) continue;
      this.replies.add(message.id);
      added += 1;
    }
    if (!last || this.state() !== 'on' || document.hasFocus()) return;
    const finished = last.title?.kind === 'agent_working' ? turnEnd(view.title) : null;
    const what = finished ?? (added > 0 ? replied(added) : null);
    if (!what) return;
    const review = view.review ? view.review.title || revisionText(view.review.revision) : 'Explore';
    // One notification per page: the next one replaces it.
    const notification = new Notification(what, { body: review, tag: 'explore' });
    notification.onclick = () => {
      window.focus();
      notification.close();
    };
  }
}

/** What the end of the agent's turn brought, as the tab's title now says it.
 * @param {TabTitle | null} title */
function turnEnd(title) {
  switch (title?.kind) {
    case 'your_turn':
      return `Question ${title.question} is ready`;
    case 'conclusion':
      return 'The conclusion is ready';
    case 'retry_needed':
      return 'The agent stopped: the round needs a retry';
    default:
      return null;
  }
}

/** @param {number} count */
function replied(count) {
  return count === 1 ? 'The agent replied in the chat' : `The agent replied ${count} times in the chat`;
}

/** The reviewer's choice in this browser, if storage allows it. */
function read() {
  try {
    return localStorage.getItem(KEY);
  } catch {
    return null;
  }
}

/** @param {'on' | 'off'} value */
function write(value) {
  try {
    localStorage.setItem(KEY, value);
  } catch {
    // Storage is off: the choice lasts as long as the page.
  }
}
