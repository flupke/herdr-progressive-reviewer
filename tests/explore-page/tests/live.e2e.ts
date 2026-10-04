import { expect } from 'e2e';
import { test } from './session.ts';

// The page changes in place as the round moves: what the reviewer is typing, and where, stays as
// it is. When the review tool cannot be reached, the page says so, waits, then shows the round
// as it is once the tool is back.

test('a reply being typed keeps its text and its focus while the implementation request goes out', async ({
  explore,
  screen,
}) => {
  await explore.open();
  await explore.conclude();
  // Exact actions: the setup is a request on its way and a reply being typed.
  await screen.getByRole('button', 'Implement 1 item').tap();
  await expect(screen.getByRole('status')).toContainText('Sending the implementation request');
  await screen.getByRole('button', 'Not ready? Reply to the agent instead').tap();
  const reply = screen.getByRole('textbox', 'Reply to the conclusion');
  await reply.fill('Keep the old name for one release.');

  await explore.deliverImplementation();
  await expect(screen.getByRole('status')).toContainText('The agent received the implementation request');
  await expect(reply).toHaveValue('Keep the old name for one release.');
  // A page loaded again would have taken the focus from the reply.
  await expect(reply).toBeFocused();
});

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
  // The page tries again with a growing delay, of a few seconds by now.
  await expect(screen.getByRole('status')).toContainText('The agent is working on your answer', { timeout: 15_000 });
  await expect(screen.getByRole('region', 'Your answer to question 1')).toContainText('Keep the draft');
  await expect(screen.getByText('Reconnecting to the review tool', { exact: false })).toBeHidden();
});
