// Chips and tags: short labels in a pill (tags.css), shared by the screens that name the same
// thing: the Door of a question, on the question screen, in the design screen's panel and on an
// earlier question; how the reviewer's kept answer relates to the first pick and to the agent's
// recommendation, in the conclusion's decisions and on an earlier question.

/** @import { DecisionTag, Door } from "./types.ts" */

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

/** The words of each tag of a kept answer, and its colour: the agent's judgement in purple, the
 * reviewer's change in accent. */
const DECISION_TAGS = {
  changed_after_first_pick: { words: 'changed after your first pick', kind: 'accent' },
  as_recommended: { words: 'as recommended', kind: 'agent' },
};

/** The tag of a kept answer. @param {DecisionTag} tag */
export function decisionTag(tag) {
  const { words, kind } = DECISION_TAGS[tag];
  return h('span', { class: `tag ${kind}` }, words);
}
