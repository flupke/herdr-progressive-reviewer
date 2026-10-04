// The previous turn: what the reviewer answered beside what the agent recorded of it (its recap,
// its follow-ups and its reply), in one block above the next question and above the conclusion.
import { expect } from 'e2e';
import { test } from './session.ts';

const RECORDED = 'The agent recorded';

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
