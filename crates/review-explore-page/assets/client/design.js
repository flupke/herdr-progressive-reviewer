// The design of the change, which the round's first turn explained: open above the first
// question, folded away in every later stage of the round.

/** @import { DesignView, DesignPartView } from "./types.ts" */

import { h, setRenderedMarkdown } from './dom.js';

/** @param {DesignView} design */
export function designSection(design) {
  return h(
    'section',
    { class: 'design', 'aria-labelledby': 'design-title' },
    h(
      'details',
      { open: design.open },
      h('summary', {}, h('h2', { id: 'design-title' }, 'Design of the change')),
      // Each part's heading is the target of its link in the masthead's design map.
      design.parts.map((part, index) => [h('h3', { id: `design-part-${index + 1}` }, part.title), partText(part)]),
    ),
  );
}

/**
 * A part's text: its thesis as the first paragraph, then the rest, until the design screen
 * draws the theses on their own.
 * @param {DesignPartView} part
 */
function partText(part) {
  const text = h('div', { class: 'markdown' });
  setRenderedMarkdown(text, part.thesis_html + part.body_html);
  return text;
}
