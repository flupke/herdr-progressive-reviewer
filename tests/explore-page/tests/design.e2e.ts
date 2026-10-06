// The design of the change is a screen of its own: it opens the round, and the reviewer opens it
// again from any later stage, at the address the rail's "Design ▾" links to.
import { expect } from 'e2e';
import { test } from './session.ts';

test('the reviewer opens the design again from a later question, then goes back to it', async ({
  explore,
  screen,
}) => {
  await explore.askQuestion();
  await explore.answerInPane();
  await explore.askQuestion();
  await explore.open();
  // Past question 1, the page shows the round's current stage.
  await expect(screen.getByRole('region', 'Question 2')).toBeVisible();

  await explore.openDesign();
  const design = screen.getByRole('region', 'Design of the change');
  await expect(design).toBeVisible();
  await expect(screen.getByRole('region', 'Question 2')).toBeHidden();

  await design.getByRole('link', 'Go to question 2').tap();
  await expect(screen.getByRole('region', 'Question 2')).toBeVisible();
  await expect(design).toBeHidden();
});
