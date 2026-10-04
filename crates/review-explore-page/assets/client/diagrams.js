// Draws each fenced diagram block of the agent's Markdown with Mermaid, in the browser, whenever
// the page builds a region that holds one. Mermaid comes from the tool, and loads only once the
// page shows a diagram. A diagram Mermaid cannot parse keeps its source as code, with Mermaid's
// message under it, and the page tells the tool, which saves the error with the question.
//
// Every diagram of the page, the design's and a question's, is drawn the same way (design
// review, finding 30): in the page's theme and font, whose colours are read from its tokens at
// each draw and again when the page turns dark or light; at its natural size when it fits its
// frame, shrunk to the frame while that keeps its text at 12 pixels or more, and otherwise at its
// natural size in a frame that scrolls sideways; a flowchart laid out left to right is drawn top
// to bottom when it does not fit; and the nodes the agent classed `new` or `changed` take the
// page's green and amber, with a legend under the drawing.

/**
 * @import { DiagramParams } from "./types.ts"
 */

/**
 * @typedef {{ source: string, block: Element, figure: HTMLElement, failed: boolean,
 *   turned: boolean, wide: number }} Diagram
 * `turned`: drawn top to bottom in place of the left to right its source asks for; `wide`: the
 * width of its left to right drawing, once drawn.
 * @typedef {{ initialize(options: object): void, parse(source: string): Promise<unknown>,
 *   render(id: string, source: string): Promise<{ svg: string }> }} Mermaid
 */

import { h, setDiagramDrawing } from './dom.js';

const FENCE = 'mermaid';
const dark = matchMedia('(prefers-color-scheme: dark)');
/** The smallest scale a diagram is shrunk to: its 14-pixel text stays at 12 pixels or more. */
const SMALLEST = 0.85;
/** A flowchart laid out left to right, which can be drawn top to bottom instead. */
const SIDEWAYS = /^(\s*(?:%%[^\n]*\n\s*)*(?:flowchart|graph)\s+)(?:LR|RL)\b/;
/** A node classed `new` or `changed`, with `:::` or a `class` statement. */
const CLASSED = /:::\s*(?:new|changed)\b|^\s*class\s+\S+\s+(?:new|changed)\s*;?\s*$/m;
const PARTICIPANT = /^\s*(?:participant|actor)\s/gm;

/** Every diagram the page found, in the order it found them. @type {Diagram[]} */
const diagrams = [];
/** @type {Promise<Mermaid> | null} */
let library = null;
/** Each draw gets its own element ID; draws run one after the other. */
let renders = 0;
let draws = Promise.resolve();
/** @type {(error: DiagramParams) => void} */
let report = () => {};
/** The address of Mermaid's script. */
let script = '';

/**
 * Says where Mermaid's script is, and where a diagram that does not parse is reported.
 * @param {string} address
 * @param {(error: DiagramParams) => void} reporter
 */
export function configureDiagrams(address, reporter) {
  script = address;
  report = reporter;
}

/**
 * Draws the diagrams under `root` that are not drawn yet.
 * @param {Element} root
 */
export function drawDiagrams(root) {
  // Forget the diagrams of regions the page rebuilt since.
  for (let index = diagrams.length - 1; index >= 0; index--) {
    const { block, figure } = /** @type {Diagram} */ (diagrams[index]);
    if (!block.isConnected && !figure.isConnected) diagrams.splice(index, 1);
  }
  const blocks = root.querySelectorAll(`.markdown pre > code.language-${FENCE}`);
  /** @type {Diagram[]} */
  const found = [];
  for (const code of blocks) {
    const block = code.parentElement;
    // A diagram that failed shows its source in its figure: that copy is not another diagram.
    if (!block || block.closest('figure.diagram') || diagrams.some((diagram) => diagram.block === block)) continue;
    const figure = document.createElement('figure');
    figure.className = 'diagram';
    const diagram = { source: code.textContent ?? '', block, figure, failed: false, turned: false, wide: 0 };
    diagrams.push(diagram);
    found.push(diagram);
  }
  if (found.length === 0) return;
  queue(found);
}

/**
 * Fits every drawn diagram to its frame again: after the window's width changed, or once a
 * screen that was hidden shows. A flowchart that no longer fits, or fits again, is drawn again.
 */
export function fitDiagrams() {
  const turning = diagrams.filter((diagram) => !fit(diagram));
  if (turning.length > 0) queue(turning);
}

/** How many queued draws have not finished. */
let pending = 0;

/**
 * Draws `some` diagrams, after the draws already queued. Until every queued draw has finished,
 * the page's body says that it is drawing (`data-diagrams="drawing"`), for a screenshot that
 * waits for the final drawings.
 * @param {Diagram[]} some
 */
function queue(some) {
  pending++;
  document.body.dataset.diagrams = 'drawing';
  draws = draws
    .then(async () => {
      const mermaid = await load();
      for (const diagram of some) await draw(mermaid, diagram);
    })
    .catch(() => {
      // Mermaid did not load: the diagrams keep their source.
    })
    .finally(() => {
      if (--pending === 0) delete document.body.dataset.diagrams;
    });
}

let resizing = 0;
addEventListener('resize', () => {
  cancelAnimationFrame(resizing);
  resizing = requestAnimationFrame(fitDiagrams);
});

/** Mermaid, loaded once; it redraws every diagram when the page's theme changes. */
function load() {
  library ??= new Promise((resolve, reject) => {
    const element = document.createElement('script');
    element.src = script;
    element.onload = () => {
      dark.addEventListener('change', () => queue([...diagrams]));
      resolve(/** @type {{ mermaid: Mermaid }} */ (/** @type {unknown} */ (window)).mermaid);
    };
    element.onerror = () => reject(new Error('Mermaid did not load'));
    document.head.append(element);
  });
  return library;
}

/**
 * @param {Mermaid} mermaid
 * @param {Diagram} diagram
 */
async function draw(mermaid, diagram) {
  if (diagram.failed || !(diagram.block.isConnected || diagram.figure.isConnected)) return;
  mermaid.initialize(configuration());
  try {
    await mermaid.parse(diagram.source);
    const source = diagram.turned ? diagram.source.replace(SIDEWAYS, '$1TD') : diagram.source;
    const { svg } = await mermaid.render(`diagram-${++renders}`, source);
    show(diagram, svg);
  } catch (error) {
    fail(diagram, error instanceof Error ? error.message : String(error));
  }
  number();
}

/** Mermaid's configuration, in the colours the page's tokens have now. */
function configuration() {
  const colours = tokens();
  const font = getComputedStyle(document.body).getPropertyValue('--font').trim() || 'system-ui, sans-serif';
  return {
    startOnLoad: false,
    // The agent's sources are untrusted: strict, Mermaid's default, encodes HTML in labels and
    // turns click directives off; said here so that no later default change loosens it.
    securityLevel: 'strict',
    theme: 'base',
    fontFamily: font,
    themeVariables: {
      fontFamily: font,
      fontSize: '14px',
      background: colours.canvas,
      primaryColor: colours.panel,
      mainBkg: colours.panel,
      actorBkg: colours.panel,
      primaryBorderColor: colours.lineStrong,
      nodeBorder: colours.lineStrong,
      actorBorder: colours.lineStrong,
      primaryTextColor: colours.ink,
      textColor: colours.ink,
      signalColor: colours.ink,
      signalTextColor: colours.ink,
      actorTextColor: colours.ink,
      labelTextColor: colours.ink,
      lineColor: colours.muted,
      actorLineColor: colours.line,
      loopTextColor: colours.muted,
      edgeLabelBackground: colours.panel,
      labelBoxBkgColor: colours.canvas,
      labelBoxBorderColor: colours.lineStrong,
      noteBkgColor: colours.warnTint,
      noteBorderColor: colours.warn,
      noteTextColor: colours.ink,
      sequenceNumberColor: colours.canvas,
    },
    themeCSS: [
      `.node.new rect, .node.new polygon, .node.new path { fill: ${colours.goodTint} !important;`,
      `stroke: ${colours.good} !important; stroke-width: 2px !important; }`,
      `.node.changed rect, .node.changed polygon, .node.changed path { fill: ${colours.warnTint} !important;`,
      `stroke: ${colours.warn} !important; stroke-width: 2px !important; }`,
    ].join(' '),
    sequence: {
      mirrorActors: false,
      actorMargin: 22,
      width: 112,
      height: 40,
      messageMargin: 30,
      diagramMarginX: 8,
      diagramMarginY: 18,
      useMaxWidth: false,
    },
    flowchart: { curve: 'basis', nodeSpacing: 30, rankSpacing: 34, padding: 10, useMaxWidth: false },
  };
}

/** The page's colours as Mermaid needs them: literal and opaque, laid over the figure's panel. */
function tokens() {
  const probe = h('span', { hidden: true });
  document.body.append(probe);
  /** @param {string} value a CSS colour */
  const resolve = (value) => {
    probe.style.color = value;
    return getComputedStyle(probe).color;
  };
  const panel = resolve('var(--panel)');
  const canvas = document.createElement('canvas');
  canvas.width = 1;
  canvas.height = 1;
  const context = canvas.getContext('2d', { willReadFrequently: true });
  /** @param {string} value */
  const opaque = (value) => {
    if (!context) return resolve(value);
    context.fillStyle = panel;
    context.fillRect(0, 0, 1, 1);
    context.fillStyle = resolve(value);
    context.fillRect(0, 0, 1, 1);
    const [red, green, blue] = context.getImageData(0, 0, 1, 1).data;
    return `#${[red, green, blue].map((channel) => (channel ?? 0).toString(16).padStart(2, '0')).join('')}`;
  };
  const colours = {
    canvas: opaque('var(--canvas)'),
    panel: opaque('var(--panel)'),
    ink: opaque('var(--ink)'),
    muted: opaque('var(--muted)'),
    line: opaque('var(--line)'),
    lineStrong: opaque('var(--line-strong)'),
    good: opaque('var(--good)'),
    goodTint: opaque('color-mix(in srgb, var(--good) 18%, var(--panel))'),
    warn: opaque('var(--warn)'),
    warnTint: opaque('color-mix(in srgb, var(--warn) 12%, var(--panel))'),
  };
  probe.remove();
  return colours;
}

/**
 * @param {Diagram} diagram
 * @param {string} svg Mermaid's drawing, which escapes the agent's labels
 */
function show(diagram, svg) {
  const drawing = h('div', { class: 'drawing' });
  setDiagramDrawing(drawing, svg);
  const caption = h(
    'figcaption',
    {},
    CLASSED.test(diagram.source)
      ? h(
          'span',
          { class: 'legend' },
          h('span', { class: 'key new' }, 'new'),
          h('span', { class: 'key changed' }, 'changed'),
          h('span', { class: 'key unchanged' }, 'unchanged'),
        )
      : null,
    h('span', { class: 'scroll-hint' }, scrollHint(diagram.source)),
  );
  diagram.figure.replaceChildren(drawing, caption);
  if (diagram.block.isConnected) diagram.block.replaceWith(diagram.figure);
  const width = natural(diagram);
  if (!diagram.turned && SIDEWAYS.test(diagram.source)) diagram.wide = width;
  if (!fit(diagram)) queue([diagram]);
}

/** "Scroll sideways · 5 participants". @param {string} source */
function scrollHint(source) {
  const participants = source.match(PARTICIPANT)?.length ?? 0;
  return participants > 0 ? `Scroll sideways · ${participants} participants` : 'Scroll sideways';
}

/** The width Mermaid laid the drawing out at. @param {Diagram} diagram */
function natural(diagram) {
  const drawing = diagram.figure.querySelector('.drawing > svg');
  return drawing instanceof SVGSVGElement ? (drawing.viewBox.baseVal?.width ?? 0) : 0;
}

/**
 * Sizes the drawing of `diagram` for its frame: its natural size when it fits, the frame's width
 * while that keeps a scale of `SMALLEST` or more, and otherwise its natural size in a frame that
 * scrolls sideways. A frame that is not shown (on the screen the page hides) waits for the next
 * fit.
 * @param {Diagram} diagram
 * @returns {boolean} false when the diagram has to be drawn again in its other direction
 */
function fit(diagram) {
  const { figure } = diagram;
  const drawing = figure.querySelector('.drawing > svg');
  if (!(drawing instanceof SVGSVGElement) || diagram.failed) return true;
  const style = getComputedStyle(figure);
  const room = figure.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight);
  if (room <= 0) return true;
  if (diagram.wide > 0) {
    const turn = diagram.wide * SMALLEST > room;
    if (turn !== diagram.turned) {
      diagram.turned = turn;
      return false;
    }
  }
  const width = natural(diagram);
  if (width <= 0) return true;
  const scale = room / width;
  drawing.style.maxWidth = 'none';
  drawing.style.width = `${scale >= 1 || scale < SMALLEST ? width : room}px`;
  figure.classList.toggle('scrolls', scale < SMALLEST);
  return true;
}

/**
 * @param {Diagram} diagram
 * @param {string} message
 */
function fail(diagram, message) {
  diagram.failed = true;
  const caption = document.createElement('figcaption');
  const title = document.createElement('p');
  title.textContent = 'Mermaid cannot draw this diagram:';
  const details = document.createElement('pre');
  details.textContent = message;
  caption.append(title, details);
  diagram.figure.classList.add('failed');
  diagram.figure.replaceChildren(diagram.block.cloneNode(true), caption);
  if (diagram.block.isConnected) diagram.block.replaceWith(diagram.figure);
  const question = diagram.figure.closest('[data-question]');
  if (question instanceof HTMLElement) {
    report({
      question: question.dataset.question ?? '',
      version: Number(question.dataset.version),
      source: diagram.source,
      message,
    });
  }
}

/** Names each drawn diagram by its place on the page: "Diagram 1", "Diagram 2"… */
function number() {
  document.querySelectorAll('figure.diagram').forEach((figure, index) => {
    figure.setAttribute('aria-label', `Diagram ${index + 1}`);
  });
}
