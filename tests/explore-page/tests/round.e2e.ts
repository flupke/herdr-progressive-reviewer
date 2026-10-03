import { expect } from 'e2e';
import { test } from './session.ts';

const WORKING = 'The agent is working…';

test('the working state is replaced by the next question when it is ready', async ({ explore, screen, agent }) => {
  await explore.open();
  const status = screen.getByRole('status');
  await expect(status).toHaveText(WORKING);

  await explore.askQuestion();
  const first = screen.getByRole('region', 'Question 1');
  await expect(first).toContainText("Should a reopened round keep the reviewer's unsent draft?");
  await expect(status).toHaveCount(0);

  // A page that shows a question does not follow an answer given in the pane...
  await explore.answerInPane();
  await expect(first).toBeVisible();
  await expect(status).toHaveCount(0);

  // ...until the reviewer loads it again. It then waits for the agent's next question. The page
  // keeps its address, `/`, once it traded the token of the address it opened at for a cookie.
  await agent.act('load the page again by navigating to its address "/"');
  await expect(status).toHaveText(WORKING);
  await expect(first).toHaveCount(0);

  await explore.askQuestion();
  const second = screen.getByRole('region', 'Question 2');
  await expect(second).toContainText('Where should the kept draft be stored?');
  await expect(second.getByRole('group', 'Choices').getByRole('radio')).toHaveCount(3);
  await expect(status).toHaveCount(0);
  await expect(first).toHaveCount(0);
});

test('the page shows the round once the agent concludes it', async ({ explore, screen }) => {
  await explore.open();
  await expect(screen.getByRole('status')).toHaveText(WORKING);

  await explore.conclude();

  const conclusion = screen.getByRole('region', 'Conclusion');
  await expect(conclusion).toContainText("A reopened round keeps the reviewer's unsent draft.");
  await expect(conclusion.getByRole('region', 'To be implemented')).toContainText('Save the draft with the round.');
  await expect(conclusion.getByRole('region', 'Future work')).toContainText('Offer to discard an old draft.');
  await expect(screen.getByRole('status')).toHaveCount(0);
});

test('the page says when no round is running', async ({ explore, screen }) => {
  await explore.open();
  await expect(screen.getByRole('status')).toHaveText(WORKING);

  await explore.reset();

  await expect(screen.getByRole('status')).toContainText('No Explore round is running.');
  await expect(screen.getByRole('region')).toHaveCount(0);
});

test('the page says when the agent is not working on its next turn', async ({ explore, screen }) => {
  await explore.open();
  await expect(screen.getByRole('status')).toHaveText(WORKING);

  await explore.interrupt();

  await expect(screen.getByRole('status')).toContainText('The agent is not working on its next turn.');
  await expect(screen.getByRole('region')).toHaveCount(0);
});
