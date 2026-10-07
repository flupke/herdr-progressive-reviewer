// The chat's width beside the page (layout.css, `--chat-width`): the width its widest message
// needs, measured when the chat opens and when a message arrives, never while the reviewer
// reads; or the width the reviewer set by dragging the chat's right edge, which this browser
// keeps until the reviewer brings back the fitted width with a double click or Enter on the
// edge. The stylesheet holds the width between the panel's and the room the page has; this
// module only says what the chat wants (`--chat-want`).

import { DESK } from './desk.js';
import { h } from './dom.js';

/** Where the browser keeps the width the reviewer set, for every round. */
const KEY = 'explore-chat-width';
/** How far an arrow key moves the edge, in pixels; with Shift, `BIG_STEP`. */
const STEP = 16;
const BIG_STEP = 64;

export class ChatWidth {
  /**
   * @param {HTMLElement} chat the chat's frame, which gets the edge
   * @param {HTMLElement} log the list of messages
   */
  constructor(chat, log) {
    this.chat = chat;
    this.log = log;
    /** The width the messages need, once measured. @type {number | null} */
    this.fitted = null;
    /** The width the reviewer set. @type {number | null} */
    this.chosen = load();
    this.edge = h('div', {
      class: 'chat-edge',
      role: 'separator',
      tabindex: 0,
      'aria-orientation': 'vertical',
      'aria-label': 'Width of the chat',
      'aria-controls': 'chat',
      title: 'Drag to set the width; double-click to fit the messages',
    });
    chat.append(this.edge);
    this.edge.addEventListener('pointerdown', (event) => this.drag(event));
    this.edge.addEventListener('dblclick', () => this.fitAgain());
    this.edge.addEventListener('keydown', (event) => this.key(event));
    this.edge.addEventListener('focus', () => this.describe());
    this.apply();
  }

  /**
   * Measures the width the widest message needs: its tables and blocks of code at the width of
   * their content, with the room the chat keeps around them. Prose wraps, and needs none.
   * @returns {boolean} whether it measured: not while the chat is hidden or a sheet
   */
  fit() {
    if (this.chat.hidden || !DESK.matches) return false;
    const wide = [...this.log.querySelectorAll('.chat-message :is(.table-frame, pre)')];
    const width = this.chat.getBoundingClientRect().width;
    const around = wide.map((element) => width - (element.parentElement?.clientWidth ?? width));
    this.chat.classList.add('measuring');
    const needs = wide.map((element, index) => element.getBoundingClientRect().width + (around[index] ?? 0));
    this.chat.classList.remove('measuring');
    this.fitted = Math.ceil(Math.max(0, ...needs));
    this.apply();
    return true;
  }

  /** Tells the stylesheet the width the chat wants: the reviewer's, or else the fitted one. */
  apply() {
    const want = this.chosen ?? this.fitted;
    const root = document.documentElement.style;
    if (want === null) root.removeProperty('--chat-want');
    else root.setProperty('--chat-want', `${Math.round(want)}px`);
  }

  /** The narrowest and the widest the chat can be in this window, as the stylesheet holds it. */
  bounds() {
    const root = document.documentElement.style;
    const want = root.getPropertyValue('--chat-want');
    // With no transition, so that each width reads at once and nothing moves after.
    const resizing = this.chat.classList.contains('resizing');
    this.chat.classList.add('resizing');
    root.setProperty('--chat-want', '0px');
    const min = this.chat.getBoundingClientRect().width;
    root.setProperty('--chat-want', '100000px');
    const max = this.chat.getBoundingClientRect().width;
    if (want) root.setProperty('--chat-want', want);
    else root.removeProperty('--chat-want');
    // Drawn back at its width before the transitions return, which would play from the widest.
    this.chat.getBoundingClientRect();
    if (!resizing) this.chat.classList.remove('resizing');
    return { min, max };
  }

  /** Gives the edge its values, for assistive technology. */
  describe() {
    if (this.chat.hidden || document.activeElement !== this.edge) return;
    const { min, max } = this.bounds();
    const now = this.chat.getBoundingClientRect().width;
    this.edge.setAttribute('aria-valuemin', String(Math.round(min)));
    this.edge.setAttribute('aria-valuemax', String(Math.round(max)));
    this.edge.setAttribute('aria-valuenow', String(Math.round(now)));
    this.edge.setAttribute('aria-valuetext', `${Math.round(now)} pixels${this.chosen === null ? ', fitted to the messages' : ''}`);
  }

  /**
   * Sets the width the reviewer chose, within the window's bounds, and keeps it for this
   * browser when `keep`.
   * @param {number} width
   * @param {{ min: number, max: number }} bounds
   * @param {boolean} keep
   */
  choose(width, bounds, keep) {
    this.chosen = Math.round(Math.min(bounds.max, Math.max(bounds.min, width)));
    this.apply();
    if (keep) save(this.chosen);
  }

  /** Brings back the width that fits the messages, and forgets the reviewer's. */
  fitAgain() {
    this.chosen = null;
    save(null);
    if (!this.fit()) this.apply();
    this.describe();
  }

  /** @param {PointerEvent} event */
  drag(event) {
    if (event.button !== 0) return;
    event.preventDefault();
    const edge = this.edge;
    edge.setPointerCapture(event.pointerId);
    const from = event.clientX;
    const start = this.chat.getBoundingClientRect().width;
    const bounds = this.bounds();
    let moved = false;
    this.chat.classList.add('resizing');
    /** @param {PointerEvent} move */
    const follow = (move) => {
      if (move.clientX === from && !moved) return;
      moved = true;
      this.choose(start + move.clientX - from, bounds, false);
    };
    const end = () => {
      edge.removeEventListener('pointermove', follow);
      edge.removeEventListener('pointerup', end);
      edge.removeEventListener('pointercancel', end);
      this.chat.classList.remove('resizing');
      if (moved && this.chosen !== null) save(this.chosen);
      this.describe();
    };
    edge.addEventListener('pointermove', follow);
    edge.addEventListener('pointerup', end);
    edge.addEventListener('pointercancel', end);
  }

  /** @param {KeyboardEvent} event */
  key(event) {
    const step = event.shiftKey ? BIG_STEP : STEP;
    const width = this.chat.getBoundingClientRect().width;
    const bounds = this.bounds();
    /** @type {Record<string, () => void>} */
    const keys = {
      ArrowLeft: () => this.choose(width - step, bounds, true),
      ArrowRight: () => this.choose(width + step, bounds, true),
      Home: () => this.choose(bounds.min, bounds, true),
      End: () => this.choose(bounds.max, bounds, true),
      Enter: () => this.fitAgain(),
    };
    const act = keys[event.key];
    if (!act) return;
    event.preventDefault();
    act();
    // Enter's fitted width describes itself.
    if (event.key !== 'Enter') this.describe();
  }
}

/** The width the reviewer set in this browser, if any. */
function load() {
  try {
    const kept = Number(localStorage.getItem(KEY));
    return kept > 0 ? kept : null;
  } catch {
    return null;
  }
}

/** Keeps the width the reviewer set in this browser, or forgets it with `null`.
 * @param {number | null} width */
function save(width) {
  try {
    if (width === null) localStorage.removeItem(KEY);
    else localStorage.setItem(KEY, String(width));
  } catch {
    // The browser keeps nothing for this page: the width lasts until the page loads again.
  }
}
