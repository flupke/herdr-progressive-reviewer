// Draws each fenced diagram block of the agent's Markdown with Mermaid, in the browser. Mermaid
// comes from the tool, and loads only when the page holds a diagram. A diagram does not follow
// the page's theme by itself: it is drawn again with Mermaid's dark theme when the page turns
// dark. A diagram Mermaid cannot parse keeps its source as code, with Mermaid's message under it,
// and the page tells the tool, which saves the error with the question.
(() => {
  const script = document.currentScript;
  const blocks = document.querySelectorAll(`.markdown pre > code.language-${script.dataset.fence}`);
  if (blocks.length === 0) return;
  const dark = matchMedia('(prefers-color-scheme: dark)');

  // Each diagram, in page order: its source, and the figure that replaces its code block.
  const diagrams = Array.from(blocks, (code, index) => {
    const figure = document.createElement('figure');
    figure.className = 'diagram';
    figure.setAttribute('aria-label', `Diagram ${index + 1}`);
    return { source: code.textContent, block: code.parentElement, figure, failed: false };
  });

  // Each render gets its own element ID. Draws run one after the other, so a theme change
  // during a draw waits for it, and a diagram fails at most once.
  let renders = 0;
  let draws = Promise.resolve();
  const redraw = () => {
    draws = draws.then(drawAll);
  };

  async function drawAll() {
    mermaid.initialize({ startOnLoad: false, theme: dark.matches ? 'dark' : 'default' });
    for (const diagram of diagrams) {
      if (diagram.failed) continue;
      try {
        await mermaid.parse(diagram.source);
        const { svg } = await mermaid.render(`diagram-${++renders}`, diagram.source);
        show(diagram, svg);
      } catch (error) {
        fail(diagram, error instanceof Error ? error.message : String(error));
      }
    }
  }

  function show(diagram, svg) {
    diagram.figure.innerHTML = svg;
    const drawing = diagram.figure.querySelector('svg');
    // At its natural size, which the frame scrolls sideways when the column is narrower: a
    // diagram shrunk to a phone's width has unreadable text.
    const width = drawing?.viewBox.baseVal?.width;
    if (width > 0) {
      drawing.style.width = `${width}px`;
      drawing.style.maxWidth = 'none';
    }
    if (diagram.block.isConnected) diagram.block.replaceWith(diagram.figure);
  }

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
    report(diagram, message);
  }

  // Tells the tool about a diagram of a question that does not parse.
  function report(diagram, message) {
    const question = diagram.figure.closest('[data-question]');
    if (!question) return;
    fetch('/diagram-errors', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        question: question.dataset.question,
        version: Number(question.dataset.version),
        source: diagram.source,
        message,
      }),
    }).catch(() => {
      // The tool is gone; the page still shows the error.
    });
  }

  const library = document.createElement('script');
  library.src = script.dataset.mermaid;
  library.onload = () => {
    redraw();
    dark.addEventListener('change', redraw);
  };
  document.head.append(library);
})();
