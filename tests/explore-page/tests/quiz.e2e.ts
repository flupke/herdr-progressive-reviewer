import { expect } from 'e2e';
import { test } from './session.ts';

const FIRST_CORRECT = 'The answer typed before closing';
const SECOND_WRONG = "In the pane's editor";

// The test picks a wrong answer on purpose: the page saying so is the expected result.
const GRADED =
  'An answer to the quiz is done once the page says whether it is correct. A pick the page calls ' +
  'not quite right is the expected result of picking that answer, not a failure: do not try again.';

test(
  'the reviewer answers the quiz one question at a time, and sees at once whether each pick is right',
  { agentContext: GRADED },
  async ({ explore, screen, agent }) => {
    await explore.open();
    await explore.concludeWithQuiz();
    // The quiz comes before the conclusion.
    await expect(screen.getByRole('region', 'Quiz')).toBeVisible();
    await expect(screen.getByRole('region', 'Conclusion')).toHaveCount(0);
    await expect(screen.getByRole('heading', 'Question 1 of 2')).toBeVisible();

    await agent.act('answer the quiz question with {answer}, then check the answer', {
      params: { answer: FIRST_CORRECT },
    });
    await expect(screen.getByRole('status')).toHaveText('Correct.');
    await expect(screen.getByText('Reopening a round no longer clears the draft', { exact: false })).toBeVisible();
    await expect(screen.getByRole('heading', 'src/drafts.rs new 7-8')).toBeVisible();
    // Why the agent thinks the item is at whiteboard level stays out of the page.
    await expect(screen.getByText('Data storage: what survives closing the pane.')).toHaveCount(0);

    await agent.act('go on to the next quiz question');
    await expect(screen.getByRole('heading', 'Question 2 of 2')).toBeVisible();
    await agent.act('answer the quiz question with {answer}, then check the answer', {
      params: { answer: SECOND_WRONG },
    });
    await expect(screen.getByRole('status')).toHaveText("Not quite. The correct answer: With the round's record");
    await expect(screen.getByText('The draft is a field of the round', { exact: false })).toBeVisible();
    expect(await explore.quiz()).toEqual({
      picks: [
        { item: 0, answer: 1, correct: true },
        { item: 1, answer: 1, correct: false },
      ],
    });

    await agent.act('go on to the conclusion');
    await expect(screen.getByRole('region', 'Conclusion')).toBeVisible();
    await expect(screen.getByRole('heading', 'Quiz: 1 of 2 correct')).toBeVisible();

    // The page opened again shows the conclusion, with the results the round saved.
    await explore.open();
    await expect(screen.getByRole('heading', 'Quiz: 1 of 2 correct')).toBeVisible();
  },
);

test('the reviewer skips the quiz and reads the conclusion', async ({ explore, screen, agent }) => {
  await explore.open();
  await explore.concludeWithQuiz();
  await expect(screen.getByRole('region', 'Quiz')).toBeVisible();

  await agent.act('skip the quiz');
  await expect(screen.getByRole('region', 'Conclusion')).toBeVisible();
  await expect(screen.getByRole('heading', 'Quiz: 0 of 2 correct, 2 skipped')).toBeVisible();
  expect(await explore.quiz()).toEqual({ skipped: true });
});
