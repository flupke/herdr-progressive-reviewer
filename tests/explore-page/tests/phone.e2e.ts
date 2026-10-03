// The page on a phone: it fits the screen, and it shows the round's current state again when
// the phone wakes.
import { expect } from 'e2e';
import { test } from './session.ts';

// What the browser of a phone that wakes, or of a tab shown again, tells the page.
function wake() {
  document.dispatchEvent(new Event('visibilitychange'));
}

test('after the phone sleeps and wakes, the page shows the current question', async ({ explore, screen, browser }) => {
  await explore.open();
  await explore.askQuestion();
  await expect(screen.getByRole('region', 'Question 1')).toBeVisible();

  // While the phone sleeps, the reviewer answers in the pane and the agent asks the next question.
  await explore.answerInPane();
  await explore.askQuestion();
  await browser.evaluate(wake);

  await expect(screen.getByRole('region', 'Question 2')).toBeVisible();
});

test('a question fits the width of the screen', async ({ explore, screen, browser }) => {
  await explore.open();
  await explore.askQuestion();
  await expect(screen.getByRole('region', 'Question 1')).toBeVisible();

  const overflow = await browser.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
  expect(overflow).toBeLessThanOrEqual(0);
});
