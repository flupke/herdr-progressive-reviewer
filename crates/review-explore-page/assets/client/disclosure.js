// A disclosure: a button that shows or hides what it controls, for an action that waits behind
// a fold ("Edit before sending", "Not ready? Reply to the agent instead", "Start a new round…").
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
      onclick: () => {
        const opened = toggle.getAttribute('aria-expanded') !== 'true';
        toggle.setAttribute('aria-expanded', String(opened));
        body.hidden = !opened;
      },
    },
    label,
  );
  return { element: h('div', { class: 'disclosure' }, toggle, body), toggle };
}
