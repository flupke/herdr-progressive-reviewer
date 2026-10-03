import { expect } from 'e2e';
import { test } from './session.ts';

const ANSWER = 'answer question 1 with "Discard the draft", write {comment} as the comment, and send it';
const COMMENT = 'Keep it for a week instead.';

test('the reviewer answers on the page, then sees the agent work and its next question', async ({
  explore,
  screen,
  agent,
}) => {
  await explore.open();
  await explore.askQuestion();

  await agent.act(ANSWER, { params: { comment: COMMENT } });
  await expect(screen.getByRole('status')).toContainText('The agent is working');
  expect(await explore.answers()).toEqual([
    { question: 'keep-draft', version: 1, choice: 'discard', comment: COMMENT },
  ]);

  await explore.askQuestion();
  await expect(screen.getByRole('region', 'Question 2')).toBeVisible();
});

test('a comment without a choice is sent as the answer', async ({ explore, screen, agent }) => {
  await explore.open();
  await explore.askQuestion();

  await agent.act('without picking a choice, write {comment} as the comment on question 1 and send it', {
    params: { comment: COMMENT },
  });
  await expect(screen.getByRole('status')).toContainText('The agent is working');
  expect(await explore.answers()).toEqual([{ question: 'keep-draft', version: 1, choice: null, comment: COMMENT }]);
});

// A browser posts the line breaks of a text area as CRLF; the pane's editor keeps LF.
const LINES = 'Keep it for a week.\nThen discard it.';

test('a comment of several lines keeps the line breaks a comment written in the pane has', async ({
  explore,
  screen,
}) => {
  await explore.open();
  await explore.askQuestion();

  // Exact actions: the comment must receive this exact text, line breaks included.
  await screen.getByRole('radio', 'Keep the draft').check();
  await screen.getByRole('textbox', 'Comment (optional)').fill(LINES);
  await screen.getByRole('button', 'Send').tap();
  await expect(screen.getByRole('status')).toContainText('The agent is working');
  expect(await explore.answers()).toEqual([{ question: 'keep-draft', version: 1, choice: 'keep', comment: LINES }]);
});

test('the page says how many lines an answer marks, and lists them on request', async ({
  explore,
  screen,
  agent,
}) => {
  await explore.open();
  await explore.askQuestion();
  await expect(screen.getByText('4 lines reviewed · 20 lines not relevant', { exact: false })).toBeVisible();

  await agent.act('open "Will mark … when you answer" to list the lines that answering the question will mark');
  await expect(screen.getByText('src/drafts.rs new 10-13', { exact: false })).toBeVisible();
});

test('an answer to a question answered in the pane meanwhile is refused', async ({ explore, screen }) => {
  await explore.open();
  await explore.askQuestion();
  // The page shows the question, and does not follow the answer given in the pane.
  await expect(screen.getByRole('region', 'Question 1')).toBeVisible();
  await explore.answerInPane();

  // An exact action: the refusal of this send is the point of the test, which a goal to
  // answer would count as a failure.
  await screen.getByRole('radio', 'Keep the draft').check();
  await screen.getByRole('button', 'Send').tap();
  // The refusal says why (not a failed delivery or a missing reply), and the page shows the round
  // as it is now.
  await expect(screen.getByRole('alert')).toContainText('this question no longer waits for an answer');
  await expect(screen.getByRole('status')).toContainText('The agent is working');
  expect(await explore.answers()).toEqual([]);
});

test('the page follows an answer cancelled in the pane, and shows a failed delivery as an error', async ({
  explore,
  screen,
  agent,
}) => {
  await explore.open();
  await explore.askQuestion();
  await agent.act(ANSWER, { params: { comment: COMMENT } });
  await expect(screen.getByRole('status')).toContainText('The agent is working');

  await explore.cancelAnswerInPane();
  await expect(screen.getByRole('region', 'Question 1')).toBeVisible();

  await agent.act(ANSWER, { params: { comment: COMMENT } });
  await expect(screen.getByRole('status')).toContainText('The agent is working');
  await explore.failDelivery();
  await expect(screen.getByRole('alert')).toContainText('The selected agent is no longer available');
});
