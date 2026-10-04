// The previous turn: what the reviewer answered beside what the agent recorded of it (its recap,
// its follow-ups and its reply), in one block above the next question and above the conclusion,
// and, when run-ahead watched the question, whether it prepared the turn while the reviewer was
// thinking, or why not.
import { expect } from 'e2e';
import { test } from './session.ts';

const RECORDED = 'The agent recorded';
const PREPARED = 'Prepared while you were thinking';
const NOT_READY = 'Not prepared: the turn for this choice was not ready yet';

test('the previous turn shows the answer beside what the agent recorded, above the next question and the conclusion', async ({
  explore,
  screen,
  browser,
}) => {
  await explore.open();
  await explore.askQuestion();
  await screen.getByRole('link', 'Go to question 1').tap();
  await expect(screen.getByRole('region', 'Question 1')).toBeVisible();
  await expect(screen.getByRole('region', RECORDED)).toHaveCount(0);

  // The reviewer answers in the pane, then opens the page again on the next question.
  await explore.answerInPane();
  await explore.askQuestion();
  await explore.open();
  const question = screen.getByRole('region', 'Question 2');
  await expect(question.getByRole('region', 'You answered Q1')).toContainText('Keep the draft');
  const recorded = question.getByRole('region', RECORDED);
  await expect(recorded).toContainText('Recorded: keep the draft.');
  await expect(recorded).toContainText('Agreed: the draft stays with the round, so reopen() keeps it.');
  // A phone hides the follow-ups, to keep the question in the first screen.
  const phone = await browser.evaluate(() => window.matchMedia('(max-width: 40rem)').matches);
  const followUps = question.getByText('Follow-ups · Say when a kept draft is older than a day.');
  if (phone) await expect(followUps).toBeHidden();
  else await expect(followUps).toBeVisible();
  // Rendered as Markdown: the backticks make a code span, the stars a bold word.
  await expect(recorded.getByText('reopen()', { exact: true })).toBeVisible();
  await expect(recorded.getByText('keep', { exact: true })).toBeVisible();
  await expect(recorded).not.toContainText('`');
  await expect(recorded).not.toContainText('**');

  await explore.answerInPane();
  await explore.conclude();
  await explore.open();
  await expect(screen.getByRole('region', 'Conclusion')).toBeVisible();
  await expect(screen.getByRole('region', RECORDED)).toContainText("Recorded: store the draft in the round's record.");
});

test('a turn prepared while the reviewer was thinking says so above the question it asks', async ({
  explore,
  screen,
}) => {
  await explore.open();
  await explore.askQuestion();
  await explore.answerInPane();
  await explore.askPreparedQuestion();
  await explore.open();
  await expect(screen.getByRole('region', 'Question 2').getByText(PREPARED)).toBeVisible();

  // The next turn the agent in the pane takes itself says nothing of the kind.
  await explore.answerInPane();
  await explore.askQuestion();
  await explore.open();
  await expect(screen.getByRole('region', 'Question 3')).toBeVisible();
  await expect(screen.getByText(PREPARED)).toHaveCount(0);
});

test('a turn the agent took itself while run-ahead watched says why no prepared turn was used', async ({
  explore,
  screen,
}) => {
  await explore.open();
  await explore.askQuestion();
  await explore.answerInPane();
  await explore.askNotPreparedQuestion();
  await explore.open();
  const question = screen.getByRole('region', 'Question 2');
  await expect(question.getByText(NOT_READY)).toBeVisible();
  await expect(question.getByText(PREPARED)).toHaveCount(0);
});
