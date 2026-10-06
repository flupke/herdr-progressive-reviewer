// The review's revision as the pane's header shows it: the shortest prefix that names it
// highlighted, then the rest of the abbreviation dimmed (style.css, "Revision"). A revision
// with no prefix, such as a Git abbreviation, shows plain.

/** @import { ShortRevision } from "./types.ts" */

import { h } from './dom.js';

/**
 * The revision as code, its prefix highlighted.
 * @param {ShortRevision} revision
 */
export function revisionCode(revision) {
  return h(
    'code',
    { class: 'revision' },
    revision.prefix ? h('span', { class: 'revision-prefix' }, revision.prefix) : null,
    revision.rest ? h('span', { class: 'revision-rest' }, revision.rest) : null,
  );
}

/**
 * The revision as plain text, for the tab's title.
 * @param {ShortRevision} revision
 */
export function revisionText(revision) {
  return revision.prefix + revision.rest;
}
