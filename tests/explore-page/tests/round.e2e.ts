import { expect } from 'e2e';
import { test } from './session.ts';

// The fixture's round starts with the agent working on its first question (question.e2e.ts checks
// that the page says so). Each test opens the page (`explore.open()` returns once it has loaded),
// then moves the round as the pane or the agent would: the page has to follow it.

test('the page follows an answer given in the pane, then shows the next question', async ({ explore, screen }) => {
  await explore.open();
  await explore.askQuestion();
  await screen.getByRole('link', 'Go to question 1').tap();
  await expect(screen.getByRole('region', 'Question 1')).toBeVisible();

  // The page follows the round in every stage, with no reload by the reviewer: an answer given
  // in the pane puts it in the working state, with the answer the reviewer may cancel.
  await explore.answerInPane();
  await expect(screen.getByRole('status')).toContainText('The agent is working');
  await expect(screen.getByRole('region', 'Your last answer')).toContainText('Keep the draft');

  await explore.askQuestion();
  await expect(screen.getByRole('region', 'Question 2')).toContainText('Where should the kept draft be stored?');
  await expect(screen.getByRole('status')).toBeHidden();
});

test('the page shows the round once the agent concludes it', async ({ explore, screen }) => {
  await explore.open();
  await explore.conclude();
  await expect(screen.getByRole('region', 'Conclusion')).toContainText("A reopened round keeps the reviewer's unsent draft.");
  // The list to be implemented is the text the reviewer edits before Implement.
  await expect(screen.getByRole('textbox', 'To be implemented')).toHaveValue('Save the draft with the round.');
  await expect(screen.getByRole('region', 'Future work')).toContainText('Offer to discard an old draft.');
  await expect(screen.getByRole('status')).toBeHidden();
});

test('the page says when no round is running', async ({ explore, screen }) => {
  await explore.open();
  await explore.reset();
  await expect(screen.getByRole('status')).toContainText('No round is running', { ignoreCase: true });
});

test("the page says when the agent's turn is paused", async ({ explore, screen }) => {
  await explore.open();
  await explore.interrupt();
  await expect(screen.getByRole('status')).toContainText('The turn is paused');
});
