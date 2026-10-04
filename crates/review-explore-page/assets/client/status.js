// The status card: one state that is not a question, as the tool maps it (StatusCard in
// src/status.rs), with its markup from docs/development.md ("The page's components") and its
// styles in status.css. Each action is a form whose submit sends the card's request.

/** @import { StatusCard, StatusAction } from "./types.ts" */

import { h } from './dom.js';

/**
 * @param {StatusCard} card
 * @returns {HTMLElement}
 */
export function statusCard(card) {
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
    card.reason ? h('p', { class: 'status-reason' }, card.code ? h('code', {}, card.reason) : card.reason) : null,
    card.kind === 'progress' ? h('div', { class: 'status-bar', 'aria-hidden': 'true' }) : null,
    next,
    card.actions.length > 0 ? h('div', { class: 'status-actions' }, card.actions.map(action)) : null,
  );
}

/** @param {StatusAction} action */
function action(action) {
  return h(
    'form',
    { class: action.method, 'data-method': action.method },
    action.fields.map((field) => h('input', { type: 'hidden', name: field.name, value: field.value })),
    h('button', { class: `button ${action.tier}`, type: 'submit' }, action.label),
    action.hint ? h('p', { class: 'hint' }, action.hint) : null,
  );
}
