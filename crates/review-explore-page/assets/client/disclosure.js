// A disclosure: a button that shows or hides what it controls, for an action that waits behind
// a fold ("Edit before sending", "Start a new round…").
// A button with `aria-expanded`, rather than `<details>`, so that every reader, and the e2e
// agent, sees a control.

import { h } from './dom.js';

let next = 0;

/**
 * @param {import('./dom.js').Children} label the button's words, or what it shows folded
 * @param {Node | Node[]} content what the button shows
 * @param {{ open?: boolean, button?: string }} [options] whether it starts open, and the
 *   button's classes (by default the muted line with its ▸)
 * @returns {{ element: HTMLElement, toggle: HTMLButtonElement }}
 */
export function disclosure(label, content, { open = false, button = 'disclosure-toggle' } = {}) {
  next += 1;
  const id = `disclosure-${next}`;
  const body = h('div', { class: 'disclosure-body', id, hidden: !open }, content);
  const toggle = h(
    'button',
    {
      class: button,
      type: 'button',
      'aria-expanded': String(open),
      'aria-controls': id,
      onclick: () => show(toggle, toggle.getAttribute('aria-expanded') !== 'true'),
    },
    label,
  );
  return { element: h('div', { class: 'disclosure' }, toggle, body), toggle };
}

/**
 * Opens the disclosure `element` (the `element` that `disclosure` returned), from a link to what
 * it hides.
 * @param {Element} element
 */
export function openDisclosure(element) {
  const toggle = element.querySelector(':scope > button[aria-controls]');
  if (toggle instanceof HTMLButtonElement) show(toggle, true);
}

/**
 * Shows or hides what `toggle` controls.
 * @param {HTMLButtonElement} toggle
 * @param {boolean} opened
 */
function show(toggle, opened) {
  toggle.setAttribute('aria-expanded', String(opened));
  const body = toggle.nextElementSibling;
  if (body instanceof HTMLElement) body.hidden = !opened;
}
