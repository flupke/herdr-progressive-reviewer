// The glossary of the change: the terms the agent defined, marked wherever the page shows them,
// in prose and in inline code but not in code blocks, diagrams or citations' code lines. A
// marked term shows its definition while the pointer rests on it or the keyboard focuses it; a
// tap opens it, as on a phone, and a second tap, Escape or a tap elsewhere closes it.
// The marks follow every change of the page: a mutation observer marks what a render adds, so
// no screen has to call it. A text a render changes in place, rather than adds, stays unmarked.

import { h } from './dom.js';

/** What a term is never marked inside: code blocks, diagrams, the citations' code lines, text
 * boxes, controls and links, the glossary's own list, and a term already marked. */
const UNMARKED = 'pre, svg, .diagram, .code-frame, textarea, input, button, a, .glossary, .term, .term-tip';

/** What may not touch a term on either side for it to count: a letter, a mark that combines
 * with one, a digit or an underscore. */
const WORD = '[\\p{L}\\p{M}\\p{N}_]';

/** A term as a pattern: its characters as they are, any space between its words.
 * @param {string} term */
function termPattern(term) {
  return term.replace(/[.*+?^${}()|[\]\\]/g, '\\$&').replace(/\s+/g, '\\s+');
}

export class Glossary {
  /** @param {HTMLElement[]} roots the parts of the page whose text the glossary marks */
  constructor(roots) {
    this.roots = roots;
    /** Each term with its definition. @type {[string, string][]} */
    this.entries = [];
    /** The terms, longest first, as one pattern. @type {RegExp | null} */
    this.pattern = null;
    this.key = '';
    /** The definition each mark shows. @type {WeakMap<HTMLElement, string>} */
    this.defined = new WeakMap();
    this.tip = h('div', { class: 'term-tip', id: 'term-tip', role: 'tooltip', hidden: true });
    document.body.append(this.tip);
    /** The mark whose definition shows. @type {HTMLElement | null} */
    this.shown = null;
    this.observer = new MutationObserver((records) => {
      for (const record of records) for (const node of record.addedNodes) this.mark(node);
      // The marks just made are additions too: they hold no term left to mark.
      this.observer.takeRecords();
    });
    for (const root of roots) this.observer.observe(root, { childList: true, subtree: true });
    document.addEventListener('click', (event) => {
      if (this.shown && !(event.target instanceof Node && this.shown.contains(event.target))) this.hide();
    });
    // Escape closes the definition alone, not the chat it may show in.
    document.addEventListener('keydown', (event) => {
      if (event.key === 'Escape' && this.shown) {
        event.preventDefault();
        this.hide();
      }
    });
    // A fixed definition follows its term when the page or the chat scrolls.
    const follow = () => {
      if (this.shown) this.place(this.shown);
    };
    addEventListener('scroll', follow, { capture: true, passive: true });
    addEventListener('resize', follow);
  }

  /** Takes `glossary`, the definition of each term, and marks the page again when it changed.
   * @param {{ [term in string]?: string }} glossary */
  set(glossary) {
    const entries = Object.entries(glossary).filter(
      /** @returns {entry is [string, string]} */ (entry) => typeof entry[1] === 'string',
    );
    const key = JSON.stringify(entries);
    if (key === this.key) return;
    this.key = key;
    this.entries = entries;
    const alternatives = entries
      .map(([term]) => term)
      .sort((a, b) => b.length - a.length)
      .map(termPattern);
    this.pattern =
      alternatives.length > 0 ? new RegExp(`(?<!${WORD})(?:${alternatives.join('|')})(?!${WORD})`, 'giu') : null;
    this.hide();
    for (const root of this.roots) {
      unmark(root);
      this.mark(root);
    }
    this.observer.takeRecords();
  }

  /** Marks each use of a term in the text under `node`. @param {Node} node */
  mark(node) {
    const pattern = this.pattern;
    if (!pattern) return;
    const start = node instanceof Element ? node : node.parentElement;
    if (!start || start.closest(UNMARKED)) return;
    /** @type {Text[]} */
    const texts = [];
    if (node instanceof Text) texts.push(node);
    else {
      const walker = document.createTreeWalker(node, NodeFilter.SHOW_TEXT, {
        acceptNode: (text) =>
          text.parentElement?.closest(UNMARKED) ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT,
      });
      for (let text = walker.nextNode(); text; text = walker.nextNode()) texts.push(/** @type {Text} */ (text));
    }
    for (const text of texts) this.markText(text, pattern);
  }

  /** @param {Text} text @param {RegExp} pattern */
  markText(text, pattern) {
    const value = text.data;
    pattern.lastIndex = 0;
    /** @type {(Node | string)[]} */
    const parts = [];
    let at = 0;
    for (const match of value.matchAll(pattern)) {
      const entry = this.entryOf(match[0]);
      if (!entry) continue;
      const index = match.index ?? 0;
      if (index > at) parts.push(value.slice(at, index));
      parts.push(this.termMark(match[0], entry));
      at = index + match[0].length;
    }
    if (parts.length === 0) return;
    if (at < value.length) parts.push(value.slice(at));
    text.replaceWith(...parts);
  }

  /** The term and definition `word` is a use of, by the pattern's own rule: the same letters
   * whatever their case, and any space between words. @param {string} word */
  entryOf(word) {
    return this.entries.find(([term]) => new RegExp(`^(?:${termPattern(term)})$`, 'iu').test(word));
  }

  /** A marked use `word` of the term of `entry`, which shows its definition.
   * @param {string} word @param {[string, string]} entry */
  termMark(word, entry) {
    const mark = h('span', { class: 'term', tabindex: 0, role: 'button', 'aria-describedby': 'term-tip' }, word);
    this.defined.set(mark, entry[1]);
    const show = () => this.show(mark);
    mark.addEventListener('mouseenter', show);
    mark.addEventListener('focus', show);
    mark.addEventListener('mouseleave', () => this.hide());
    mark.addEventListener('blur', () => this.hide());
    // A second tap closes the definition the first opened; a mouse, which shows it on hover,
    // keeps it open while it rests there.
    let open = false;
    mark.addEventListener('pointerdown', (event) => {
      open = event.pointerType !== 'mouse' && this.shown === mark;
    });
    mark.addEventListener('click', (event) => {
      // A term inside a choice's label opens its definition, not the choice; the click still
      // reaches the page, which closes what it has open.
      event.preventDefault();
      if (open) this.hide();
      else show();
      open = false;
    });
    return mark;
  }

  /** Shows the definition of the term `mark` holds, alone: the mark beside it names the term,
   * and a screen reader reads the definition after that name. @param {HTMLElement} mark */
  show(mark) {
    const definition = this.defined.get(mark);
    if (definition === undefined) return;
    this.shown = mark;
    this.tip.replaceChildren(definition);
    this.place(mark);
  }

  /** Puts the definition under `mark`, or above it near the bottom of the window.
   * @param {HTMLElement} mark */
  place(mark) {
    // Measured at the window's corner, where nothing narrows it.
    this.tip.style.left = '0px';
    this.tip.style.top = '0px';
    this.tip.hidden = false;
    const box = mark.getBoundingClientRect();
    const width = this.tip.offsetWidth;
    const height = this.tip.offsetHeight;
    const left = Math.max(8, Math.min(box.left, window.innerWidth - width - 8));
    const below = box.bottom + 6 + height < window.innerHeight;
    this.tip.style.left = `${left}px`;
    this.tip.style.top = `${below ? box.bottom + 6 : box.top - 6 - height}px`;
  }

  hide() {
    this.shown = null;
    this.tip.hidden = true;
  }
}

/** The glossary's list, at the end of the design: each term with its definition, in alphabetical
 * order; nothing when the agent defined no term.
 * @param {{ [term in string]?: string }} glossary */
export function glossaryList(glossary) {
  const entries = Object.entries(glossary)
    .filter(/** @returns {entry is [string, string]} */ (entry) => typeof entry[1] === 'string')
    .sort(([a], [b]) => a.localeCompare(b, undefined, { sensitivity: 'base' }));
  if (entries.length === 0) return null;
  return h(
    'section',
    { class: 'glossary', 'aria-labelledby': 'glossary-title' },
    h('h2', { class: 'eyebrow', id: 'glossary-title' }, 'Glossary'),
    h(
      'dl',
      {},
      entries.map(([term, definition]) => [h('dt', {}, term), h('dd', {}, definition)]),
    ),
  );
}

/** Takes the marks off under `root`, leaving their text. @param {HTMLElement} root */
function unmark(root) {
  for (const mark of root.querySelectorAll('.term')) {
    const parent = mark.parentNode;
    mark.replaceWith(mark.textContent ?? '');
    parent?.normalize();
  }
}
