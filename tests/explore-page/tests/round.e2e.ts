// The page follows the round as the pane and the agent move it, with no reload by the reviewer,
// and keeps what the reviewer is typing.
import { expect } from 'e2e';
import { test } from './session.ts';

test('the page follows an answer given in the pane, the next question, then the conclusion', async ({
  explore,
  screen,
}) => {
  await explore.open();
  await explore.askQuestion();
  const design = screen.getByRole('region', 'Design of the change');
  await expect(design).toBeVisible();

  // The design that opened the round gives way to the round once it moves on in the pane; the
  // answer shows, which the reviewer may cancel.
  await explore.answerInPane();
  await expect(screen.getByRole('status')).toContainText('The agent is working');
  await expect(design).toBeHidden();
  await expect(screen.getByRole('region', 'Your answer to question 1')).toContainText('Keep the draft');

  await explore.askQuestion();
  await expect(screen.getByRole('region', 'Question 2')).toContainText('Where should the kept draft be stored?');
  await expect(screen.getByRole('status')).toBeHidden();

  await explore.answerInPane();
  await explore.conclude();
  await expect(screen.getByRole('region', 'Conclusion')).toContainText("A reopened round keeps the reviewer's unsent draft.");
  await expect(screen.getByRole('textbox', 'To be implemented')).toHaveValue('Save the draft with the round.');
});

test('a comment being typed comes back when the page follows the round back to its question', async ({
  explore,
  screen,
}) => {
  await explore.open();
  await explore.askQuestion();
  await screen.getByRole('link', 'Go to question 1').tap();
  await screen.getByRole('textbox', 'Comment · optional').fill('Keep it, but log the overflow.');

  // The pane moves the round away from the question, then back.
  await explore.answerInPane();
  await expect(screen.getByRole('status')).toContainText('The agent is working');
  await explore.cancelAnswerInPane();
  await expect(screen.getByRole('region', 'Question 1')).toBeVisible();
  await expect(screen.getByRole('textbox', 'Comment · optional')).toHaveValue('Keep it, but log the overflow.');
});
