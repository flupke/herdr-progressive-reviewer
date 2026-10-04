import { expect } from 'e2e';
import { test, type Session } from './session.ts';

// The standalone server's second question explains where a kept draft goes with a Mermaid
// diagram, then carries a second diagram that Mermaid cannot parse. The agent asks both questions
// before the page opens. The design of the change, folded above the question, draws its own
// diagram first: the question's two are Diagram 2 and Diagram 3.
async function openQuestion2(explore: Session) {
  await explore.askQuestion();
  await explore.askQuestion();
  await explore.open();
}

test('a question draws its diagram, with Mermaid served by the tool', async ({ explore, screen, browser, app }) => {
  // The host of every request the page makes.
  const hosts = new Set<string>();
  await browser.route('**', async (route) => {
    hosts.add(new URL(route.request.url).host);
    await route.continue();
  });
  await openQuestion2(explore);
  // Mermaid drew it: only the drawing shows a box's label on its own (a diagram that fails keeps
  // its source as one block of code). The label alone would also show in a drawing with no size,
  // so the figure's own visibility is checked too.
  const figure = screen.getByRole('figure', 'Diagram 2');
  await expect(figure.getByText('Editor')).toBeVisible();
  await expect(figure).toBeVisible();
  expect([...hosts]).toEqual([new URL(app.baseUrl!).host]);
});

test('at phone width, a wide diagram keeps its natural size and scrolls sideways', async ({
  explore,
  screen,
  browser,
}) => {
  await browser.setViewport({ width: 390, height: 844 });
  await openQuestion2(explore);
  await expect(screen.getByRole('figure', 'Diagram 2')).toBeVisible();

  const size = await browser.evaluate(() => {
    const figure = document.querySelector('figure[aria-label="Diagram 2"]')!;
    const svg = figure.querySelector('svg')!;
    return {
      natural: svg.viewBox.baseVal.width,
      drawn: svg.getBoundingClientRect().width,
      frame: figure.clientWidth,
      scrolls: figure.scrollWidth,
      page: document.documentElement.scrollWidth - window.innerWidth,
    };
  });
  // The fixed diagram is wider than a phone's column...
  expect(size.natural).toBeGreaterThan(size.frame);
  // ...and is drawn at the size Mermaid laid it out at, not shrunk to the column...
  expect(Math.abs(size.drawn - size.natural)).toBeLessThan(1);
  // ...in a frame that scrolls sideways, while the page itself does not.
  expect(size.scrolls).toBeGreaterThan(size.frame);
  expect(size.page).toBeLessThanOrEqual(0);
});

test('a diagram that does not parse shows its source and the error, and the tool keeps it', async ({
  explore,
  screen,
}) => {
  await openQuestion2(explore);
  // The figure keeps the source, and shows Mermaid's message.
  const failed = screen.getByRole('figure', 'Diagram 3');
  await expect(failed).toContainText('draft --> record[saved (with the answers)]');
  await expect(failed).toContainText('Parse error');
  await expect
    .poll(() => explore.diagramErrors(), { timeout: 10_000 })
    .toEqual([
      expect.objectContaining({
        question: 'draft-storage',
        version: 1,
        source: expect.stringContaining('flowchart'),
        message: expect.stringContaining('Parse error'),
      }),
    ]);
});
