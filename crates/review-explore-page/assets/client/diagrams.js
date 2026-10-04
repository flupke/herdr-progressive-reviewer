// Draws each fenced diagram block of the agent's Markdown with Mermaid, in the browser, whenever
// the page builds a region that holds one. Mermaid comes from the tool, and loads only once the
// page shows a diagram. A diagram is drawn again with Mermaid's dark theme when the page turns
// dark. A diagram Mermaid cannot parse keeps its source as code, with Mermaid's message under it,
// and the page tells the tool, which saves the error with the question.

/**
 * @import { DiagramParams } from "./types.ts"
 */

/**
 * @typedef {{ source: string, block: Element, figure: HTMLElement, failed: boolean }} Diagram
 * @typedef {{ initialize(options: object): void, parse(source: string): Promise<unknown>,
 *   render(id: string, source: string): Promise<{ svg: string }> }} Mermaid
 */

import { setDiagramDrawing } from './dom.js';

const FENCE = 'mermaid';
const dark = matchMedia('(prefers-color-scheme: dark)');

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
    if (!block || diagrams.some((diagram) => diagram.block === block)) continue;
    const figure = document.createElement('figure');
    figure.className = 'diagram';
    const diagram = { source: code.textContent ?? '', block, figure, failed: false };
    diagrams.push(diagram);
    found.push(diagram);
  }
  if (found.length === 0) return;
  draws = draws.then(async () => {
    const mermaid = await load();
    for (const diagram of found) await draw(mermaid, diagram);
  });
}

/** Mermaid, loaded once; it redraws every diagram when the page's theme changes. */
function load() {
  library ??= new Promise((resolve, reject) => {
    const element = document.createElement('script');
    element.src = script;
    element.onload = () => {
      dark.addEventListener('change', () => {
        draws = draws.then(async () => {
          const mermaid = await load();
          for (const diagram of diagrams) await draw(mermaid, diagram);
        });
      });
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
  // The agent's sources are untrusted: strict, Mermaid's default, encodes HTML in labels and
  // turns click directives off; said here so that no later default change loosens it.
  mermaid.initialize({ startOnLoad: false, securityLevel: 'strict', theme: dark.matches ? 'dark' : 'default' });
  try {
    await mermaid.parse(diagram.source);
    const { svg } = await mermaid.render(`diagram-${++renders}`, diagram.source);
    show(diagram, svg);
  } catch (error) {
    fail(diagram, error instanceof Error ? error.message : String(error));
  }
  number();
}

/**
 * @param {Diagram} diagram
 * @param {string} svg Mermaid's drawing, which escapes the agent's labels
 */
function show(diagram, svg) {
  setDiagramDrawing(diagram.figure, svg);
  const drawing = diagram.figure.querySelector('svg');
  // At its natural size, which the frame scrolls sideways when the column is narrower: a
  // diagram shrunk to a phone's width has unreadable text.
  const width = drawing?.viewBox.baseVal?.width ?? 0;
  if (drawing && width > 0) {
    drawing.style.width = `${width}px`;
    drawing.style.maxWidth = 'none';
  }
  if (diagram.block.isConnected) diagram.block.replaceWith(diagram.figure);
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
