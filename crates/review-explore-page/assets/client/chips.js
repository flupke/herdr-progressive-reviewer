// Chips: short labels in an outlined pill (tags.css), shared by the screens that name the same
// thing: the Door of a question, on the question screen and in the design screen's panel.

/** @import { Door } from "./types.ts" */

import { h } from './dom.js';

/** Each door kind's chip: its name and its colour. */
export const DOORS = {
  one_way: { name: 'One-way door', kind: 'warn' },
  two_way: { name: 'Two-way door', kind: 'good' },
  mixed: { name: 'Mixed door', kind: 'neutral' },
  unknown: { name: 'Door unknown', kind: 'neutral' },
};

/** The chip of a question's Door. @param {Door} door */
export function doorChip(door) {
  const { name, kind } = DOORS[door];
  return h('span', { class: `chip ${kind}` }, name);
}
