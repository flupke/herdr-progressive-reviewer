// Cited lines: each citation with its note and its lines as rows of the diff, colored by the
// tool (each token's colour role is a class, `c-<role>`, in citations.css). The question shows
// its citations most decisive first: the first one, then the others folded until the reviewer
// opens them. A quiz item's proof uses the same citation.

/** @import { CitationView, RowView } from "./types.ts" */

import { h } from './dom.js';

/**
 * One citation; `id` names its heading.
 * @param {CitationView} citation
 * @param {string} id
 */
export function citation(citation, id) {
  return h(
    'section',
    { class: 'citation', 'aria-labelledby': id },
    h('h4', { id }, h('code', {}, citation.location)),
    h('p', { class: 'notes' }, citation.notes),
    citation.limitation !== null
      ? h('p', { class: 'limitation' }, citation.limitation)
      : h(
          'div',
          { class: 'code', tabindex: 0, role: 'group', 'aria-label': `Lines of ${citation.location}` },
          h('table', {}, h('tbody', {}, citation.rows.map(row))),
        ),
  );
}

const SIGNS = { added: '+', removed: '−', context: ' ' };

/** @param {RowView} row */
function row(row) {
  return h(
    'tr',
    { class: row.kind },
    h('td', { class: 'number' }, row.old_line ?? ''),
    h('td', { class: 'number' }, row.new_line ?? ''),
    h('td', { class: 'sign' }, SIGNS[row.kind]),
    h('td', { class: 'line' }, tokens(row)),
  );
}

/** A row's tokens: a span for each coloured one, and one text for each run of plain ones, as
 * the browser would parse them from markup.
 * @param {RowView} row */
function tokens(row) {
  /** @type {(HTMLElement | string)[]} */
  const nodes = [];
  for (const token of row.tokens) {
    const last = nodes.length - 1;
    if (token.role) nodes.push(h('span', { class: `c-${token.role}` }, token.text));
    else if (typeof nodes[last] === 'string') nodes[last] += token.text;
    else nodes.push(token.text);
  }
  return nodes;
}

/**
 * The question's citations, or `null` when it has none.
 * @param {CitationView[]} citations
 * @param {number} number the question's number
 */
export function citationsSection(citations, number) {
  const [first, ...others] = citations;
  if (!first) return null;
  const id = (/** @type {number} */ index) => `question-${number}-citation-${index}`;
  return h(
    'section',
    { class: 'citations', 'aria-labelledby': `question-${number}-citations` },
    h('h3', { id: `question-${number}-citations` }, 'Citations'),
    citation(first, id(1)),
    others.length > 0
      ? h(
          'details',
          {},
          h('summary', {}, `${others.length} more citation${others.length > 1 ? 's' : ''}`),
          others.map((other, index) => citation(other, id(index + 2))),
        )
      : null,
  );
}
