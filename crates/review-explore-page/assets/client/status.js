// The status card: one state that is not a question, as the tool maps it (StatusCard in
// src/status.rs), with its markup from docs/development.md ("The page's components") and its
// styles in status.css. Each action is a form whose submit sends the card's request.
//
// In the flow, the card holds its actions in a row. In a panel, where the primary action comes
// last at full width, the panel draws the card without them (`panelCard`), then puts them where
// they belong (`panelActions`).

/** @import { StatusCard, StatusAction, CardTime } from "./types.ts" */

import { h } from './dom.js';

/**
 * @param {StatusCard} card
 * @returns {HTMLElement}
 */
export function statusCard(card) {
  return drawn(card, true);
}

/**
 * `card` without its actions, for a panel, which draws them with `panelActions`.
 * @param {StatusCard} card
 * @returns {HTMLElement}
 */
export function panelCard(card) {
  return drawn(card, false);
}

/**
 * @param {StatusCard} card
 * @param {boolean} actions whether the card holds its actions
 * @returns {HTMLElement}
 */
function drawn(card, actions) {
  const title = `${card.id}-title`;
  const next =
    card.imperative || card.next
      ? h(
          'p',
          { class: 'status-next' },
          card.imperative ? h('strong', {}, card.imperative) : null,
          card.imperative && card.next ? ` ${card.next}` : card.next,
        )
      : null;
  return h(
    'div',
    { class: `status-card ${card.kind}`, id: card.id, role: card.role, 'aria-labelledby': title },
    h('span', { class: 'status-glyph', 'aria-hidden': 'true' }),
    h('p', { class: 'status-title', id: title }, card.title),
    card.reason || card.time || card.since
      ? h(
          'p',
          { class: 'status-reason' },
          card.time ? `${time(card.time)}${card.reason ? ' ' : ''}` : null,
          card.reason && card.code ? h('code', {}, card.reason) : card.reason,
          card.since ? [card.reason ? ' ' : null, since(card.since)] : null,
        )
      : null,
    card.kind === 'progress' ? h('div', { class: 'status-bar', 'aria-hidden': 'true' }) : null,
    next,
    actions && card.actions.length > 0
      ? h('div', { class: 'status-actions' }, card.actions.map((one) => action(one, '')))
      : null,
  );
}

/**
 * The actions of `card` as a panel holds them: one under the other at its full width, each
 * with its hint under it.
 * @param {StatusCard} card
 * @returns {HTMLElement[]}
 */
export function panelActions(card) {
  return card.actions.map((one) => action(one, ' block'));
}

/**
 * @param {StatusAction} action
 * @param {string} width ` block` for the full width of a panel
 */
function action(action, width) {
  return h(
    'form',
    { class: action.method, 'data-method': action.method },
    action.fields.map((field) => h('input', { type: 'hidden', name: field.name, value: field.value })),
    h('button', { class: `button ${action.tier}${width}`, type: 'submit' }, action.label),
    action.hint ? h('p', { class: 'hint' }, action.hint) : null,
  );
}

/**
 * "14:36": the time `ms`, milliseconds since the epoch, in the reader's clock.
 * @param {number} ms
 */
export function clockTime(ms) {
  return new Date(ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hourCycle: 'h23' });
}

/**
 * When a card's state began, in the reader's clock: "Sent at 14:36.", with the day when it is
 * not today.
 * @param {CardTime} at
 */
function time(at) {
  const date = new Date(at.ms);
  const clock = clockTime(at.ms);
  const today = new Date().toDateString() === date.toDateString();
  const day = today ? '' : ` on ${date.toLocaleDateString([], { day: 'numeric', month: 'short' })}`;
  return `${at.words} ${clock}${day}.`;
}

/**
 * The time since what a card waits for began, counted in the page: "Sent 0:42 ago". The count
 * goes on every second, quietly for a screen reader, which would otherwise read the card again
 * at each change.
 * @param {CardTime} at
 */
function since(at) {
  ticker ??= setInterval(tick, 1000);
  return [
    `${at.words} `,
    h('span', { class: 'status-elapsed', 'data-since': at.ms, 'aria-live': 'off' }, elapsed(at.ms)),
    ' ago',
  ];
}

/** The timer that counts every card's time since, while one shows. @type {number | null} */
let ticker = null;

/** Counts on every time since the page shows, and stops once it shows none. */
function tick() {
  const counts = document.querySelectorAll('.status-elapsed[data-since]');
  if (counts.length === 0 && ticker !== null) {
    clearInterval(ticker);
    ticker = null;
  }
  for (const count of counts) {
    if (count instanceof HTMLElement) count.textContent = elapsed(Number(count.dataset.since));
  }
}

/**
 * The time from `ms` to now: "0:42", "12:05", or "1:02:03" past an hour.
 * @param {number} ms milliseconds since the epoch
 */
function elapsed(ms) {
  const seconds = Math.max(0, Math.floor((Date.now() - ms) / 1000));
  const two = (/** @type {number} */ value) => String(value).padStart(2, '0');
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor(seconds / 60) % 60;
  return hours > 0 ? `${hours}:${two(minutes)}:${two(seconds % 60)}` : `${minutes}:${two(seconds % 60)}`;
}
