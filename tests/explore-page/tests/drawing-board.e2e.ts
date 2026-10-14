// Back to the drawing board: in place of an answer, the reviewer sends the agent back to the
// drawing board from the design screen, the answer's panel or the masthead's menu, behind a
// confirmation; the agent then concludes with the changes the design needs.
import { expect } from 'e2e';
import { test } from './session.ts';

const SENT_BACK = { question: 'keep-draft', version: 1, choice: null, comment: '', drawing_board: true };

test('the reviewer sends the agent back to the drawing board from the design screen, then takes it back', async ({
  explore,
  screen,
}) => {
  await explore.open();
  await explore.askQuestion();
  const design = screen.getByRole('region', 'Design of the change');

  // The first click turns the button into its confirmation, and sends nothing yet.
  await design.getByRole('button', 'Back to the drawing board…').tap();
  await expect(design.getByRole('button', 'Back to the drawing board…')).toHaveCount(0);
  expect(await explore.answers()).toEqual([]);
  await design.getByRole('button', 'Confirm: back to the drawing board').tap();
  await expect(screen.getByRole('status')).toContainText('The agent is working on your answer to question 1');
  await expect(screen.getByRole('region', 'Your answer to question 1')).toContainText('Back to the drawing board');
  expect(await explore.answers()).toEqual([SENT_BACK]);

  await explore.conclude();
  await expect(screen.getByRole('textbox', 'To be implemented')).toBeVisible();
  await expect(screen.getByRole('region', 'Your decisions')).toContainText('Back to the drawing board');

  // Cancel answer takes it back, as any answer: the question waits again.
  await screen.getByRole('button', 'Cancel this answer…').tap();
  await screen.getByRole('button', 'Confirm: cancel my answer').tap();
  await expect(screen.getByRole('button', 'Send answer')).toBeVisible();
});

test('the reviewer sends the agent back to the drawing board from the menu, or under the answer', async ({
  explore,
  screen,
}) => {
  await explore.open();
  await explore.askQuestion();
  await screen.getByRole('link', 'Go to question 1').tap();
  const question = screen.getByRole('region', 'Question 1');
  // Under the answer, Escape takes the confirmation back.
  await question.getByRole('button', 'Back to the drawing board…').tap();
  await question.getByRole('button', 'Confirm: back to the drawing board').press('Escape');
  await expect(question.getByRole('button', 'Back to the drawing board…')).toBeVisible();
  await expect(question.getByRole('button', 'Confirm: back to the drawing board')).toHaveCount(0);

  await screen.getByRole('button', 'Round menu').tap();
  await screen.getByRole('button', 'Back to the drawing board… ends the questions').tap();
  await screen.getByRole('button', 'Confirm: back to the drawing board').tap();
  await expect(screen.getByRole('status')).toContainText('The agent is working on your answer to question 1');
  expect(await explore.answers()).toEqual([SENT_BACK]);
  await expect(screen.getByRole('button', 'Confirm: back to the drawing board')).toHaveCount(0);
});
