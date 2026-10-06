// The conclusion's quiz, one item at a time before the conclusion: Check grades the pick, then
// the reviewer goes on to the next item, then to the conclusion with the score.
import { expect } from 'e2e';
import { test } from './session.ts';

const FIRST_CORRECT = 'The answer typed before closing';
// e2e cannot build a locator from a name with an apostrophe: the pattern stands in for it.
const SECOND_WRONG = /^In the pane.s editor$/;

test('the reviewer picks one right answer and one wrong one, then reads the conclusion with the score', async ({
  explore,
  screen,
}) => {
  await explore.open();
  await explore.concludeWithQuiz();
  const first = screen.getByRole('region', 'Quiz · Question 1 of 2');
  await expect(first).toBeVisible();
  await expect(screen.getByRole('region', 'Conclusion')).toHaveCount(0);

  const answers = screen.getByRole('group', 'Answers');
  await answers.getByRole('radio', FIRST_CORRECT).check();
  await screen.getByRole('button', 'Check').tap();
  await expect(screen.getByRole('status')).toContainText('Correct.');
  await screen.getByRole('button', 'Next question').tap();

  await expect(screen.getByRole('region', 'Quiz · Question 2 of 2')).toBeVisible();
  await answers.getByRole('radio', SECOND_WRONG).check();
  await screen.getByRole('button', 'Check').tap();
  await expect(screen.getByRole('status')).toContainText('Not quite.');
  expect(await explore.quiz()).toEqual({
    picks: [
      { item: 0, answer: 1, correct: true },
      { item: 1, answer: 1, correct: false },
    ],
  });

  await screen.getByRole('button', 'Show the conclusion').tap();
  await expect(screen.getByRole('region', 'Conclusion')).toBeVisible();
  await expect(screen.getByRole('heading', 'Quiz · 1 of 2 correct')).toBeVisible();

  // The page opened again shows the conclusion, with the results the round saved.
  await explore.open();
  await expect(screen.getByRole('heading', 'Quiz · 1 of 2 correct')).toBeVisible();
});

test('the reviewer skips the quiz, reads the conclusion, then the answers', async ({ explore, screen }) => {
  await explore.open();
  await explore.concludeWithQuiz();
  await expect(screen.getByRole('region', 'Quiz · Question 1 of 2')).toBeVisible();

  await screen.getByRole('button', 'Skip the quiz').tap();
  await expect(screen.getByRole('region', 'Conclusion')).toBeVisible();
  await expect(screen.getByRole('heading', 'Quiz · 0 of 2 correct, 2 skipped')).toBeVisible();
  expect(await explore.quiz()).toEqual({ skipped: true });

  await screen.getByRole('link', 'See the answers').tap();
  const item = screen.getByRole('region', 'Question 1');
  await expect(item).toContainText('Skipped.');
  await expect(item.getByRole('listitem').filter({ hasText: 'Correct' })).toContainText(FIRST_CORRECT);
});
