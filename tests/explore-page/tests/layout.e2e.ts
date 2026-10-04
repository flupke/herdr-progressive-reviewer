// The page uses the width of a desktop window: the design's diagram and table show whole beside
// the design map, a paragraph keeps a readable line length, and a question's choices sit beside
// its explanation. The phone's single column is checked in phone.e2e.ts and diagram.e2e.ts.
import { expect } from 'e2e';
import { test } from './session.ts';

/** The width of the single column that the page used to keep at every size: 44rem. */
const SINGLE_COLUMN = 44 * 16;

test('on a desktop window, the design and the question use the width of the window', async ({
  explore,
  screen,
  browser,
}) => {
  // The width the reviewer's design handoff draws the page at.
  await browser.setViewport({ width: 1440, height: 900 });
  await explore.open();
  await explore.askQuestion();
  const design = screen.getByRole('region', 'Design of the change');
  // The design's sequence diagram is drawn: a participant's label shows in the drawing.
  await expect(design.getByRole('figure', 'Diagram 1').getByText('Round record').first()).toBeVisible();
  await expect(design.getByRole('table')).toBeVisible();

  // Sizes have no locator: they are read in the page, from the parts the checks above found.
  const designed = await browser.evaluate(() => {
    const box = (selector: string) => document.querySelector(selector)!.getBoundingClientRect();
    const figure = document.querySelector('figure[aria-label="Diagram 1"]')!;
    const table = document.querySelector('.design-screen .table-frame')!;
    return {
      content: box('main').width,
      figure: { frame: figure.clientWidth, drawing: figure.scrollWidth },
      table: { frame: table.clientWidth, content: table.scrollWidth },
      paragraph: box('.design-part .markdown > p').width,
      parts: box('.design-part'),
      map: box('.design-nav'),
      page: document.documentElement.scrollWidth - window.innerWidth,
    };
  });
  // The content is wider than the old column...
  expect(designed.content).toBeGreaterThan(SINGLE_COLUMN * 1.5);
  // ...so the diagram and the table that fit the window show whole, with nothing to scroll...
  expect(designed.figure.frame).toBeGreaterThan(SINGLE_COLUMN);
  expect(designed.figure.drawing).toBeLessThanOrEqual(designed.figure.frame);
  expect(designed.table.content).toBeLessThanOrEqual(designed.table.frame);
  // ...while a paragraph keeps a readable line length...
  expect(designed.paragraph).toBeLessThanOrEqual(SINGLE_COLUMN);
  // ...and the design map takes the panel beside the parts: 416 pixels wide.
  expect(designed.map.left).toBeGreaterThanOrEqual(designed.parts.right);
  expect(designed.map.width).toBe(416);
  expect(designed.page).toBeLessThanOrEqual(0);

  await screen.getByRole('link', 'Go to question 1').tap();
  await expect(screen.getByRole('region', 'Question 1')).toBeVisible();
  const asked = await browser.evaluate(() => {
    const box = (selector: string) => document.querySelector(selector)!.getBoundingClientRect();
    return {
      explanation: box('.question .explanation'),
      choices: box('.question .choices'),
      panel: box('.question .panel'),
      page: document.documentElement.scrollWidth - window.innerWidth,
    };
  });
  // The choices sit beside the explanation, not under it, in the panel of the question.
  expect(asked.choices.left).toBeGreaterThanOrEqual(asked.explanation.right);
  expect(asked.choices.top).toBeLessThan(asked.explanation.bottom);
  expect(asked.panel.width).toBe(416);
  expect(asked.page).toBeLessThanOrEqual(0);
});
