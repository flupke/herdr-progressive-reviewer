// What the agent said back to the reviewer's previous answer (its recap, its follow-ups and its
// reply) shows above its next question and above its conclusion, as in the pane.
import { expect } from 'e2e';
import { test } from './session.ts';

const REPLY = 'Reply from the agent';

test("the agent's reply to the previous answer shows above the next question and the conclusion", async ({
  explore,
  screen,
}) => {
  await explore.open();
  await explore.askQuestion();
  await screen.getByRole('link', 'Go to question 1').tap();
  await expect(screen.getByRole('region', 'Question 1')).toBeVisible();
  await expect(screen.getByRole('region', REPLY)).toHaveCount(0);

  // The reviewer answers in the pane, then opens the page again on the next question.
  await explore.answerInPane();
  await explore.askQuestion();
  await explore.open();
  const reply = screen.getByRole('region', REPLY);
  await expect(reply).toContainText('Recorded: keep the draft.');
  await expect(reply).toContainText('Follow-up: Say when a kept draft is older than a day.');
  await expect(reply).toContainText('Agreed: the draft stays with the round, so reopen() keeps it.');
  // Rendered as Markdown: the backticks make a code span, the stars a bold word.
  await expect(reply.getByText('reopen()', { exact: true })).toBeVisible();
  await expect(reply.getByText('keep', { exact: true })).toBeVisible();
  await expect(reply).not.toContainText('`');
  await expect(reply).not.toContainText('**');
  const headings = await screen.getByRole('heading', { level: 2 }).allTextContents();
  expect(headings.indexOf(REPLY)).toBeLessThan(headings.indexOf('Question 2'));

  await explore.answerInPane();
  await explore.conclude();
  await explore.open();
  await expect(screen.getByRole('region', 'Conclusion')).toBeVisible();
  await expect(screen.getByRole('region', REPLY)).toContainText("Recorded: store the draft in the round's record.");
  const concluded = await screen.getByRole('heading', { level: 2 }).allTextContents();
  expect(concluded.indexOf(REPLY)).toBeLessThan(concluded.indexOf('Conclusion'));
});
