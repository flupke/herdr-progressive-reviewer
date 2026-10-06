// When the review tool cannot be reached, the page says so, waits, then shows the round as it is
// once the tool is back.
import { expect } from 'e2e';
import { test } from './session.ts';

test('the page waits while the reviewer restarts, then shows the round as it is now', async ({ explore, screen }) => {
  await explore.open();
  await explore.askQuestion();
  await screen.getByRole('link', 'Go to question 1').tap();
  await expect(screen.getByRole('region', 'Question 1')).toBeVisible();

  await explore.restartReviewer();
  await expect(screen.getByText('Reconnecting to the review tool', { exact: false })).toBeVisible();
  // Actions wait until the tool is back.
  await expect(screen.getByRole('button', 'Send answer')).toBeDisabled();

  await explore.answerInPane();
  await explore.reviewerBack();
  // The page tries again with a growing delay, capped at 10 seconds (client/socket.js).
  await expect(screen.getByRole('status')).toContainText('The agent is working on your answer', { timeout: 15_000 });
  await expect(screen.getByRole('region', 'Your answer to question 1')).toContainText('Keep the draft');
  await expect(screen.getByText('Reconnecting to the review tool', { exact: false })).toBeHidden();
});
