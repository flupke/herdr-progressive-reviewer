// Cited lines: each citation with its note, then its lines as rows of the diff in a code frame
// whose head names the file and the lines, colored by the tool (each token's colour role is a
// class, `c-<role>`, in citations.css). An added or removed row is marked by a bar in the
// gutter. The question shows its citations most decisive first: the first one, then the others
// folded until the reviewer opens them. A quiz item's proof uses the same citation.

/** @import { CitationView, RowView } from "./types.ts" */

import { disclosure } from './disclosure.js';
import { h } from './dom.js';

/**
 * One citation; `id` names its heading, the head of its code frame.
 * @param {CitationView} citation
 * @param {string} id
 */
export function citation(citation, id) {
  const added = citation.rows.length > 0 && citation.rows.every((row) => row.kind === 'added');
  return h(
    'section',
    { class: 'citation', 'aria-labelledby': id },
    h('p', { class: 'notes' }, citation.notes),
    h(
      'div',
      { class: 'code-frame' },
      h('h4', { class: 'code-head', id }, h('code', {}, citation.path), ' ', h('span', {}, citation.span)),
      citation.limitation !== null
        ? h('p', { class: 'limitation' }, citation.limitation)
        : h(
            'div',
            { class: 'code', tabindex: 0, role: 'group', 'aria-label': `Lines of ${citation.location}` },
            h('table', { class: added ? 'all-added' : null }, h('tbody', {}, citation.rows.map(row))),
          ),
    ),
  );
}

const SIGNS = { added: '+', removed: '−', context: ' ' };

/** @param {RowView} row */
function row(row) {
  return h(
    'tr',
    { class: row.kind },
    h('td', { class: 'number old' }, row.old_line ?? ''),
    h('td', { class: 'number new' }, row.new_line ?? ''),
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

/** Whether the reviewer opened the citations after the first, for each question by its number
 * and its citations: the question's screen and the screen of its answer, read again while
 * the agent works, show them the same way. @type {Map<string, boolean>} */
const opened = new Map();

/**
 * The question's citations, or `null` when it has none: the first one, then the others behind a
 * fold, open when the reviewer left it open on another screen of the same question.
 * @param {CitationView[]} citations
 * @param {number} number the question's number
 */
export function citationsSection(citations, number) {
  const [first, ...others] = citations;
  if (!first) return null;
  const id = (/** @type {number} */ index) => `question-${number}-citation-${index}`;
  const citationsOf = (/** @type {number} */ many) => `citation${many > 1 ? 's' : ''}`;
  const key = `${number}:${citations.map((each) => each.location).join(' ')}`;
  const more =
    others.length > 0
      ? disclosure(
          `${others.length} more ${citationsOf(others.length)}`,
          others.map((other, index) => citation(other, id(index + 2))),
          { open: opened.get(key) ?? false, onToggle: (open) => opened.set(key, open) },
        )
      : null;
  return h(
    'section',
    { class: 'citations', 'aria-labelledby': `question-${number}-citations` },
    h('h3', { class: 'eyebrow', id: `question-${number}-citations` }, 'Citations'),
    citation(first, id(1)),
    more?.element ?? null,
  );
}
