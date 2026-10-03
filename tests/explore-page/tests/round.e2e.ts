import { expect } from 'e2e';
import { test } from './session.ts';

// The fixture's round starts with the agent working on its first question (question.e2e.ts checks
// that the page says so). Each test opens the page (`explore.open()` returns once it has loaded),
// then moves the round: the page has to follow it.

test('the working state is replaced by the next question when it is ready', async ({ explore, screen, agent }) => {
  await explore.open();
  await explore.askQuestion();
  await expect(screen.getByRole('region', 'Question 1')).toBeVisible();

  // A page that shows a question does not follow an answer given in the pane: for a second, it
  // does not turn back to the working state (a page that polled would within half a second)...
  await explore.answerInPane();
  await expect(screen.getByRole('status')).not.toBeVisible();
  await expect(screen.getByRole('region', 'Question 1')).toBeVisible();

  // ...until the reviewer loads it again. It then waits for the agent's next question. The page
  // keeps its address, `/`, once it traded the token of the address it opened at for a cookie.
  await agent.act('load the page again at "/"');
  await expect(screen.getByRole('status')).toContainText('The agent is working');

  await explore.askQuestion();
  await expect(screen.getByRole('region', 'Question 2')).toBeVisible();
  await agent.assert(
    'the page shows question 2, which asks where the kept draft should be stored and offers three ' +
      'choices; it shows no other question and no longer says that the agent is working',
  );
});

test('the page shows the round once the agent concludes it', async ({ explore, screen, agent }) => {
  await explore.open();
  await explore.conclude();
  await expect(screen.getByRole('region', 'Conclusion')).toBeVisible();
  await agent.assert(
    "the conclusion says that a reopened round keeps the reviewer's unsent draft, lists saving the " +
      'draft with the round as to be implemented, and offering to discard an old draft as future ' +
      'work; the page no longer says that the agent is working',
  );
});

test('the page says when no round is running', async ({ explore, screen }) => {
  await explore.open();
  await explore.reset();
  await expect(screen.getByRole('status')).toContainText('No Explore round is running');
});

test('the page says when the agent is not working on its next turn', async ({ explore, screen }) => {
  await explore.open();
  await explore.interrupt();
  await expect(screen.getByRole('status')).toContainText('The agent is not working on its next turn');
});
