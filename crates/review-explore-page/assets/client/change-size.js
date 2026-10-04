// The size of a change, as the meter's window and the start cover say it: "+125 −10", the
// added lines in green and the removed in red (meter.css), and "in 4 files".

/** @import { MarkTally } from "./types.ts" */

import { h } from './dom.js';

/**
 * "+125 −10", from the changed lines of `tally`.
 * @param {MarkTally} tally
 */
export function changeSize(tally) {
  const changed = tally.change.changed;
  return h(
    'span',
    { class: 'change-size' },
    h('span', { class: 'added' }, `+${changed.lines_added}`),
    ' ',
    h('span', { class: 'removed' }, `−${changed.lines_removed}`),
  );
}

/** "4 files", "1 file". @param {number} count */
export function fileCount(count) {
  return count === 1 ? '1 file' : `${count} files`;
}
