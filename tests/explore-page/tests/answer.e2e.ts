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
  await screen.getByRole('link', 'Go to question 1').tap();

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
  await screen.getByRole('link', 'Go to question 1').tap();

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
  await screen.getByRole('link', 'Go to question 1').tap();

  // Exact actions: the comment must receive this exact text, line breaks included.
  await screen.getByRole('radio', 'Keep the draft').check();
  await screen.getByRole('textbox', 'Comment · optional').fill(LINES);
  await screen.getByRole('button', 'Send answer').tap();
  await expect(screen.getByRole('status')).toContainText('The agent is working');
  expect(await explore.answers()).toEqual([{ question: 'keep-draft', version: 1, choice: 'keep', comment: LINES }]);
});

test('the page says what an answer marks and the share of the change it leaves reviewed, and lists the lines on request', async ({
  explore,
  screen,
  agent,
}) => {
  await explore.open();
  await explore.askQuestion();
  await screen.getByRole('link', 'Go to question 1').tap();
  const answer = screen.getByRole('form', 'Your answer to question 1');
  // Lines marked not relevant count as reviewed, as the meter counts them; the list of lines
  // keeps the split.
  await expect(answer).toContainText('Answering marks 24 lines reviewed');
  // The fixture's change has 3 changed lines, one of which Jev marked when the round started:
  // the answer marks the other two of them, which brings the share from 33% to 100%.
  await expect(answer).toContainText('33% → 100%');

  await agent.act('open the line that says what answering marks, to list the lines it marks');
  await expect(screen.getByText('src/drafts.rs new 10-13 (reviewed)')).toBeVisible();
  await expect(screen.getByText('tests/drafts.rs new 1-20 (not relevant', { exact: false })).toBeVisible();
});

test('an answer to a question answered in the pane meanwhile is refused', async ({ explore, screen }) => {
  await explore.open();
  await explore.askQuestion();
  await screen.getByRole('link', 'Go to question 1').tap();
  // The page shows the question, and is held there while the pane answers it.
  await expect(screen.getByRole('region', 'Question 1')).toBeVisible();
  await explore.holdPage();
  await explore.answerInPane();

  // An exact action: the refusal of this send is the point of the test, which a goal to
  // answer would count as a failure.
  await screen.getByRole('radio', 'Keep the draft').check();
  await screen.getByRole('button', 'Send answer').tap();
  // The refusal says why (not a failed delivery or a missing reply), and the page shows the round
  // as it is now.
  await expect(screen.getByRole('alert')).toContainText('Question 1 was already answered');
  await expect(screen.getByRole('status')).toContainText('The agent is working');
  expect(await explore.answers()).toEqual([]);
});

// Two goals: a recording of both takes longer than the default deadline. The second answer
// starts from the comment the first one left in the box.
test(
  'the page follows an answer cancelled in the pane, and shows a failed delivery as an error',
  {
    timeout: 60_000,
    agentContext:
      'Answering question 1 on this page is done once the page says that the agent is working; until then the answer is not sent, even when its comment is already in the box.',
  },
  async ({ explore, screen, agent }) => {
    await explore.open();
    await explore.askQuestion();
    await screen.getByRole('link', 'Go to question 1').tap();
    await agent.act(ANSWER, { params: { comment: COMMENT } });
    await expect(screen.getByRole('status')).toContainText('The agent is working');

    await explore.cancelAnswerInPane();
    await expect(screen.getByRole('region', 'Question 1')).toBeVisible();

    await agent.act(ANSWER, { params: { comment: COMMENT } });
    await expect(screen.getByRole('status')).toContainText('The agent is working');
    await explore.failDelivery();
    await expect(screen.getByRole('alert')).toContainText('The selected agent is no longer available');
  },
);
