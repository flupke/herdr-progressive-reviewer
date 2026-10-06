// The client's rendering rules, in one place (.agents/wiki/explore-page-client.md):
//
// 1. One state, one entry point: the latest view from the tool is the only model, and
//    `render(view)` in page.js draws it. What only the page knows (a notice, the quiz item just
//    answered) lives beside it and is never overwritten by a push.
// 2. Stable regions: the page is a fixed list of regions, each marked by an anchor in its parent.
//    A region is rebuilt only when its key changes; with the same key it stays as it is, so the
//    nodes the reviewer uses (a text box, a selection, an open fold) survive every push.
// 3. Text from the data goes in with `textContent` (through `h`); HTML only through two named
//    functions: `setRenderedMarkdown`, for the agent's Markdown the tool rendered, and
//    `setDiagramDrawing`, for the SVG Mermaid drew in strict mode. The agent's plain texts
//    (a choice, its reason, a follow-up) go through `codeSpans`, which draws their backtick
//    spans as code, still as text nodes.
// 4. A text box's value is set only when it is built, from the reviewer's draft (drafts.js), and
//    never on a push. A rebuild that takes the focus from a text box gives it back to the text
//    box of the same draft in the new nodes, with its selection.

/**
 * @typedef {Record<string, string | number | boolean | null | undefined | EventListener>} Props
 */

/**
 * @typedef {Node | string | number | null | undefined | false} Leaf
 * @typedef {Leaf | Children[]} Children
 */

/**
 * An element: `h('p', { class: 'hint' }, 'Text')`. A prop that is `false`, `null` or `undefined`
 * is left out, `true` sets an empty attribute, and a function prop starting with `on` listens to
 * the event. Children that are strings become text nodes; `false`, `null` and `undefined` are
 * skipped, and arrays, nested or not, are flattened.
 * @template {keyof HTMLElementTagNameMap} K
 * @param {K} tag
 * @param {Props} [props]
 * @param {...Children} children
 * @returns {HTMLElementTagNameMap[K]}
 */
export function h(tag, props = {}, ...children) {
  const element = document.createElement(tag);
  for (const [name, value] of Object.entries(props)) {
    if (value === false || value === null || value === undefined) continue;
    if (typeof value === 'function') {
      element.addEventListener(name.slice(2), value);
    } else {
      element.setAttribute(name, value === true ? '' : String(value));
    }
  }
  append(element, children);
  return element;
}

/**
 * @param {Element} element
 * @param {Children} children
 */
function append(element, children) {
  if (Array.isArray(children)) {
    for (const child of children) append(element, child);
  } else if (children !== null && children !== undefined && children !== false) {
    element.append(typeof children === 'number' ? String(children) : children);
  }
}

/**
 * The agent's plain `text`, such as a choice or its reason, with each Markdown code span
 * (`` `name` ``) drawn as code: text nodes and `<code class="inline-code">` elements, never
 * HTML. A span follows Markdown's rule: a run of backticks opens it and the next run of as many
 * closes it, a line break inside is a space, and one space inside each end goes when both ends
 * have one. A run that nothing closes stays as it is written, and so does other Markdown.
 * @param {string} text
 * @returns {(string | HTMLElement)[]}
 */
export function codeSpans(text) {
  /** @type {(string | HTMLElement)[]} */
  const nodes = [];
  let written = 0;
  let at = text.indexOf('`');
  while (at >= 0) {
    const width = runLength(text, at);
    const close = closingRun(text, at + width, width);
    if (close < 0) {
      at = text.indexOf('`', at + width);
      continue;
    }
    if (at > written) nodes.push(text.slice(written, at));
    nodes.push(h('code', { class: 'inline-code' }, spanText(text.slice(at + width, close))));
    written = close + width;
    at = text.indexOf('`', written);
  }
  if (written < text.length) nodes.push(text.slice(written));
  return nodes;
}

/** How many backticks run from `at`, a run's length.
 * @param {string} text
 * @param {number} at */
function runLength(text, at) {
  let end = at;
  while (text[end] === '`') end += 1;
  return end - at;
}

/** Where the first run of exactly `width` backticks from `from` starts, or -1.
 * @param {string} text
 * @param {number} from
 * @param {number} width */
function closingRun(text, from, width) {
  for (let at = text.indexOf('`', from); at >= 0; ) {
    const run = runLength(text, at);
    if (run === width) return at;
    at = text.indexOf('`', at + run);
  }
  return -1;
}

/** The text of a code span between its backticks.
 * @param {string} inside */
function spanText(inside) {
  const line = inside.replace(/\r\n|\r|\n/g, ' ');
  return line.startsWith(' ') && line.endsWith(' ') && /[^ ]/.test(line) ? line.slice(1, -1) : line;
}

/**
 * The one place where HTML goes into the page: the agent's Markdown, which the tool rendered
 * (and which shows raw HTML as text). A `<script>` inserted this way never runs, and the content
 * security policy forbids inline scripts anyway. Each table is put in a frame that scrolls.
 * @param {Element} element
 * @param {string} html
 */
export function setRenderedMarkdown(element, html) {
  element.innerHTML = html;
  // Each table in a frame of its own, which scrolls sideways when the table is wider than its
  // column (markdown.css).
  for (const table of element.querySelectorAll('table')) {
    const frame = document.createElement('div');
    frame.className = 'table-frame';
    table.replaceWith(frame);
    frame.append(table);
  }
}

/**
 * The other place where HTML goes into the page: the SVG that Mermaid drew from a diagram's
 * source, in its strict mode, which encodes the agent's labels and turns click directives off.
 * @param {Element} figure
 * @param {string} svg
 */
export function setDiagramDrawing(figure, svg) {
  figure.innerHTML = svg;
}

/** A `div.markdown` holding the agent's rendered Markdown.
 * @param {string} html
 * @param {string} [extra] more classes
 */
export function markdown(html, extra) {
  const element = h('div', { class: extra ? `markdown ${extra}` : 'markdown' });
  setRenderedMarkdown(element, html);
  return element;
}

/**
 * A region of a parent element: the nodes it shows sit just before its anchor, a comment, so
 * that regions keep their order and the stylesheet's sibling selectors still see the nodes as
 * siblings. `show` rebuilds the nodes only when the key changed.
 */
export class Region {
  /**
   * @param {Element} parent
   * @param {string} name names the anchor, for a reader of the page's DOM
   */
  constructor(parent, name) {
    this.anchor = document.createComment(name);
    parent.append(this.anchor);
    /** @type {Node[]} */
    this.nodes = [];
    /** @type {string | undefined} */
    this.key = undefined;
    /** @type {unknown} the component the region shows, when it shows one */
    this.current = undefined;
  }

  /**
   * Shows what `build` makes for `key`, unless the region shows that key already. A `null` key
   * empties the region.
   * @param {string | null} key
   * @param {() => Node | Node[] | null} build
   * @returns {boolean} whether the region was rebuilt
   */
  show(key, build) {
    if (key === null) {
      this.clear();
      return false;
    }
    if (key === this.key) return false;
    keepingFocus(() => {
      this.clear();
      const built = build();
      this.nodes = built === null ? [] : Array.isArray(built) ? built : [built];
      for (const node of this.nodes) this.anchor.before(node);
    });
    this.key = key;
    return true;
  }

  /**
   * Shows the component that `create` makes for `key`, unless the region shows that key
   * already, then returns it, so that the caller updates its parts in place.
   * @template {{ element: Node }} C
   * @param {string} key
   * @param {() => C} create
   * @returns {C}
   */
  component(key, create) {
    this.show(key, () => {
      const component = create();
      this.current = component;
      return component.element;
    });
    return /** @type {C} */ (this.current);
  }

  clear() {
    for (const node of this.nodes) node.parentNode?.removeChild(node);
    this.nodes = [];
    this.key = undefined;
    this.current = undefined;
  }
}

/** The key of a region that shows `data`: it changes whenever the data does.
 * @param {unknown} data
 */
export function keyOf(data) {
  return JSON.stringify(data);
}

/**
 * Runs `change`, which may replace the node that has the focus, and gives the focus back to the
 * text box of the same draft (its `data-draft`), with its selection and scroll.
 * @param {() => void} change
 */
function keepingFocus(change) {
  const focused = document.activeElement;
  const draft = focused instanceof HTMLTextAreaElement ? focused.dataset.draft : undefined;
  if (!draft || !(focused instanceof HTMLTextAreaElement)) {
    change();
    return;
  }
  const { selectionStart, selectionEnd, selectionDirection, scrollTop } = focused;
  change();
  if (focused.isConnected) return;
  const again = document.querySelector(`textarea[data-draft="${CSS.escape(draft)}"]`);
  if (!(again instanceof HTMLTextAreaElement)) return;
  again.focus({ preventScroll: true });
  again.setSelectionRange(selectionStart, selectionEnd, selectionDirection ?? undefined);
  again.scrollTop = scrollTop;
}
