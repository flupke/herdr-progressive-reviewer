import { expect } from 'e2e';
import { test } from './session.ts';

test('the page shows the question the agent posts, and the reviewer picks a choice', async ({
  explore,
  screen,
  agent,
}) => {
  // The page opens while the agent works, and follows the round when the question comes.
  await explore.open();
  await expect(screen.getByRole('status')).toContainText('The agent is working');
  await explore.askQuestion();
  const question = screen.getByRole('region', 'Question 1');
  await expect(question).toContainText("Should a reopened round keep the reviewer's unsent draft?");
  await expect(screen.getByRole('status')).not.toBeVisible();
  // Beside the agent's choices, which the steps below pick, the page offers None of the above.
  await expect(question.getByRole('radio', 'None of the above')).toBeVisible();

  await agent.act('pick "Discard the draft" as the answer to question 1');
  await expect(screen.getByRole('radio', 'Discard the draft')).toBeChecked();
});
