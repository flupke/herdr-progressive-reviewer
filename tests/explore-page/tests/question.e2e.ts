import { expect } from 'e2e';
import { test } from './session.ts';

test('the page shows the question the agent posts, and the reviewer picks a choice', async ({
  explore,
  screen,
  agent,
}) => {
  await explore.open();
  await expect(screen.getByRole('status')).toHaveText('The agent is working…');

  await explore.askQuestion();

  const question = screen.getByRole('region', 'Question 1');
  await expect(question).toContainText("Should a reopened round keep the reviewer's unsent draft?");
  const choices = question.getByRole('group', 'Choices');
  for (const choice of ['Keep the draft', 'Discard the draft', 'None of the above']) {
    await expect(choices.getByRole('radio', choice)).toBeVisible();
  }
  await expect(choices.getByRole('radio')).toHaveCount(3);
  await expect(screen.getByRole('status')).toHaveCount(0);

  await agent.act('pick "Discard the draft" as the answer to question 1');
  await expect(choices.getByRole('radio', 'Discard the draft')).toBeChecked();
});
