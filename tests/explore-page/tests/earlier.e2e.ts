// Earlier questions: each done question on the round rail opens the question as the reviewer
// answered it, read only, with the way back to the round.
import { expect } from 'e2e';
import { test } from './session.ts';

test('the reviewer reads question 1 again from the rail, then goes back to question 2', async ({ explore, screen }) => {
  await explore.open();
  await explore.askQuestion();
  await screen.getByRole('link', 'Go to question 1').tap();
  await explore.answerInPane();
  await explore.askQuestion();
  await expect(screen.getByRole('region', 'Question 2')).toBeVisible();

  await screen.getByRole('navigation', 'Round').getByRole('link', 'Q1').tap();
  const earlier = screen.getByRole('region', 'Question 1 · answered');
  const answer = earlier.getByRole('region', 'Your answer to question 1');
  await expect(answer.getByRole('radio', 'Keep the draft')).toBeChecked();
  // Nothing on it can change the round: no comment to write, nothing to send.
  await expect(earlier.getByRole('group', 'Choices')).toBeDisabled();
  await expect(earlier.getByRole('textbox')).toHaveCount(0);
  await expect(earlier.getByRole('button', 'Send answer')).toHaveCount(0);

  await answer.getByRole('link', 'Go to question 2').tap();
  await expect(screen.getByRole('region', 'Question 2')).toBeVisible();
  await expect(earlier).toBeHidden();
  expect(await explore.answers()).toEqual([]);
});
