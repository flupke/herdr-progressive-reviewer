// Starting a round from the page. The fixture's round starts with the agent working: each test
// resets it first, so that the page shows that no round is running and offers to start one.
import { expect } from 'e2e';
import { test } from './session.ts';

test('the reviewer starts a round, reads its design, then goes on to question 1', async ({ explore, screen }) => {
  await explore.open();
  await explore.reset();

  await screen.getByRole('button', 'Start').tap();
  await expect(screen.getByRole('status')).toContainText('Preparing the round');
  expect(await explore.starts()).toEqual([{ challenger: false }]);

  // The review tool starts the round afterwards, out of the page's hands.
  await explore.sendKickoff();
  await expect(screen.getByRole('status')).toContainText('The agent is working');
  await explore.askQuestion();
  // The round opens on its design, which leads to question 1.
  const design = screen.getByRole('region', 'Design of the change');
  await expect(design).toBeVisible();
  await expect(screen.getByRole('region', 'Question 1')).toBeHidden();

  await design.getByRole('link', 'Go to question 1').tap();
  await expect(screen.getByRole('region', 'Question 1')).toBeVisible();
  await expect(design).toBeHidden();
});

test('the reviewer starts a round with the Challenger', async ({ explore, screen }) => {
  await explore.open();
  await explore.reset();

  await screen.getByRole('button', 'Start with Challenger').tap();
  await expect(screen.getByRole('status')).toContainText('Preparing the round');
  expect(await explore.starts()).toEqual([{ challenger: true }]);
});
