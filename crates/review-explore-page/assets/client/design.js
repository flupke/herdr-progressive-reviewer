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
      design.parts.map((part) => [h('h3', {}, part.title), partText(part)]),
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
