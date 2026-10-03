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
  await expect(screen.getByRole('region', 'Question 1')).toBeVisible();
  // An absence is an exact fact: a judge reading a long page may not establish it.
  await expect(screen.getByRole('status')).not.toBeVisible();
  await agent.assert(
    "question 1 asks whether a reopened round should keep the reviewer's unsent draft, and offers " +
      'three choices: keep the draft, discard the draft, and none of the above',
  );

  await agent.act('pick "Discard the draft" as the answer to question 1');
  await expect(screen.getByRole('radio', 'Discard the draft')).toBeChecked();
});
