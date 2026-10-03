import { expect } from 'e2e';
import { test } from './session.ts';

// The standalone server's first question explains itself with a table whose cells carry status
// marks, a Tip callout, raw HTML, and folded Door and Blast radius sections.
test('a question shows its explanation, and its Door section on request', async ({ explore, screen, agent }) => {
  await explore.open();
  await explore.askQuestion();
  // The page loads the question once its script sees it; a judgement does not wait.
  await expect(screen.getByRole('region', 'Question 1')).toBeVisible();

  await agent.assert(
    'question 1 explains its choices with a table whose cells are marked good, bad or warning, and with a Tip callout',
  );
  // The agent's raw HTML shows as the text it typed.
  await expect(screen.getByText('A draft keeps what the reviewer typed, such as <b>tags</b>, as plain text.')).toBeVisible();

  const door = screen.getByText('Two-way — Removing the field later drops only unsent drafts.');
  await expect(door).toBeHidden();
  await agent.act('open the "Door" section of question 1');
  await expect(door).toBeVisible();
});
