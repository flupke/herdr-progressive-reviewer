// Answering a question on the page: what the reviewer picks and writes is what the session
// receives.
import { expect } from 'e2e';
import { test } from './session.ts';

const COMMENT = 'Keep it for a week.\nThen discard it.';

test('the reviewer answers on the page, then sees the agent work and its next question', async ({
  explore,
  screen,
}) => {
  await explore.open();
  // The page opens while the agent works, and follows the round when the question comes.
  await expect(screen.getByRole('status')).toContainText('The agent is working');
  await explore.askQuestion();
  await screen.getByRole('link', 'Go to question 1').tap();
  const question = screen.getByRole('region', 'Question 1');
  await expect(question).toContainText("Should a reopened round keep the reviewer's unsent draft?");

  await question.getByRole('radio', 'Discard the draft').check();
  await question.getByRole('textbox', 'Comment · optional').fill(COMMENT);
  await question.getByRole('button', 'Send answer').tap();
  await expect(screen.getByRole('status')).toContainText('The agent is working on your answer to question 1');
  expect(await explore.answers()).toEqual([
    { question: 'keep-draft', version: 1, choice: 'discard', comment: COMMENT },
  ]);
  // The panel holds what was sent, the choice selected among the question's choices.
  const sent = screen.getByRole('region', 'Your answer to question 1');
  await expect(sent.getByRole('radio', 'Discard the draft')).toBeChecked();

  await explore.askQuestion();
  await expect(screen.getByRole('region', 'Question 2')).toBeVisible();
});

test('a comment without a choice is sent as the answer', async ({ explore, screen }) => {
  await explore.open();
  await explore.askQuestion();
  await screen.getByRole('link', 'Go to question 1').tap();
  const question = screen.getByRole('region', 'Question 1');

  await question.getByRole('textbox', 'Comment · optional').fill('Keep it for a week instead.');
  await question.getByRole('button', 'Send answer').tap();
  await expect(screen.getByRole('status')).toContainText('The agent is working');
  expect(await explore.answers()).toEqual([
    { question: 'keep-draft', version: 1, choice: null, comment: 'Keep it for a week instead.' },
  ]);
});
