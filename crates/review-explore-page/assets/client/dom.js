// The client's rendering rules, in one place (docs/development.md, "The page's client"):
//
// 1. One state, one entry point: the latest view from the tool is the only model, and
//    `render(view)` in page.js draws it. What only the page knows (a notice, the quiz item just
//    answered) lives beside it and is never overwritten by a push.
// 2. Stable regions: the page is a fixed list of regions, each marked by an anchor in its parent.
//    A region is rebuilt only when its key changes; with the same key it stays as it is, so the
//    nodes the reviewer uses (a text box, a selection, an open fold) survive every push.
// 3. Text from the data goes in with `textContent` (through `h`); HTML only through two named
//    functions: `setRenderedMarkdown`, for the agent's Markdown the tool rendered, and
//    `setDiagramDrawing`, for the SVG Mermaid drew in strict mode.
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
 * The one place where HTML goes into the page: the agent's Markdown, which the tool rendered
 * (and which shows raw HTML as text). A `<script>` inserted this way never runs, and the content
 * security policy forbids inline scripts anyway.
 * @param {Element} element
 * @param {string} html
 */
export function setRenderedMarkdown(element, html) {
  element.innerHTML = html;
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
