// The page uses the width of a desktop window: the design's diagram and table show whole, a
// paragraph keeps a readable line length, and a question's choices sit beside its explanation.
// The phone's single column is checked in phone.e2e.ts and diagram.e2e.ts.
import { expect } from 'e2e';
import { test } from './session.ts';

/** The width of the single column that the page used to keep at every size: 44rem. */
const SINGLE_COLUMN = 44 * 16;

test('on a desktop window, the design and the question use the width of the window', async ({
  explore,
  screen,
  browser,
}) => {
  await browser.setViewport({ width: 1280, height: 800 });
  await explore.open();
  await explore.askQuestion();
  const design = screen.getByRole('region', 'Design of the change');
  // The design's sequence diagram is drawn: a participant's label shows in the drawing.
  await expect(design.getByRole('figure', 'Diagram 1').getByText('Round record').first()).toBeVisible();
  await expect(design.getByRole('table')).toBeVisible();

  // Sizes have no locator: they are read in the page, from the parts the checks above found.
  const measured = await browser.evaluate(() => {
    const box = (selector: string) => document.querySelector(selector)!.getBoundingClientRect();
    const figure = document.querySelector('figure[aria-label="Diagram 1"]')!;
    const table = document.querySelector('.design table')!;
    return {
      content: box('main').width,
      figure: { frame: figure.clientWidth, drawing: figure.scrollWidth },
      table: { frame: table.clientWidth, content: table.scrollWidth },
      paragraph: box('.design .markdown > p').width,
      explanation: box('.question .explanation'),
      choices: box('.question .choices'),
      page: document.documentElement.scrollWidth - window.innerWidth,
    };
  });
  // The content is wider than the old column...
  expect(measured.content).toBeGreaterThan(SINGLE_COLUMN * 1.5);
  // ...so the diagram and the table that fit the window show whole, with nothing to scroll...
  expect(measured.figure.frame).toBeGreaterThan(SINGLE_COLUMN);
  expect(measured.figure.drawing).toBeLessThanOrEqual(measured.figure.frame);
  expect(measured.table.content).toBeLessThanOrEqual(measured.table.frame);
  // ...while a paragraph keeps a readable line length...
  expect(measured.paragraph).toBeLessThanOrEqual(SINGLE_COLUMN);
  // ...and the choices sit beside the explanation, not under it.
  expect(measured.choices.left).toBeGreaterThanOrEqual(measured.explanation.right);
  expect(measured.choices.top).toBeLessThan(measured.explanation.bottom);
  expect(measured.page).toBeLessThanOrEqual(0);
});
