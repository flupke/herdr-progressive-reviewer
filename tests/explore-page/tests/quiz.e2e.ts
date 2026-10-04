// The conclusion's quiz, one item at a time before the conclusion: the item as the headline, and
// in the panel the answers and Check; after Check, the verdict where the reviewer is looking, each
// answer's mark in words beside its colour, the proof in the reading column, and one next step.
// The rail's quiz step names the item shown, then the score.
import { expect } from 'e2e';
import { test } from './session.ts';

const FIRST_CORRECT = 'The answer typed before closing';
const SECOND_WRONG = "In the pane's editor";
const SECOND_CORRECT = "With the round's record";

// The test picks a wrong answer on purpose: the page saying so is the expected result.
const GRADED =
  'An answer to the quiz is done once the page says whether it is correct. A pick the page calls ' +
  'not quite right is the expected result of picking that answer, not a failure: do not try again.';

test(
  'the reviewer picks one right answer and one wrong one, reads each verdict and proof, then the conclusion',
  { agentContext: GRADED },
  async ({ explore, screen, agent, browser }) => {
    // Whether the verdict of the item just checked is in the window, with no scroll by the
    // reviewer. A position has no locator: it is read in the page.
    const verdictInView = () =>
      browser.evaluate(() => {
        const box = document.querySelector('[data-shows="quiz"]')!.getBoundingClientRect();
        return box.top >= 0 && box.bottom <= window.innerHeight;
      });

    await explore.open();
    await explore.concludeWithQuiz();
    const rail = screen.getByRole('navigation', 'Round');
    // The quiz comes before the conclusion, its item as the headline; the proof waits for Check.
    const first = screen.getByRole('region', 'Quiz · Question 1 of 2');
    await expect(first.getByRole('heading', { level: 2 })).toContainText('What does the answer box show?');
    await expect(screen.getByRole('region', 'Conclusion')).toHaveCount(0);
    await expect(screen.getByRole('region', 'Proof')).toHaveCount(0);
    await expect(rail.getByText('Quiz 1/2')).toHaveAttribute('aria-current', 'step');

    await agent.act('answer the quiz question with {answer}, then check the answer', {
      params: { answer: FIRST_CORRECT },
    });
    // The verdict says it in words, with why, where the reviewer is looking.
    await expect(screen.getByRole('status')).toHaveText(
      'Correct. Reopening a round no longer clears the draft that the round keeps.',
    );
    expect(await verdictInView()).toBe(true);
    // The pick is the correct answer: its card carries both marks, as words.
    const picked = screen.getByRole('list', 'Answers').getByRole('listitem').filter({ hasText: 'Your pick' });
    await expect(picked).toContainText(FIRST_CORRECT);
    await expect(picked).toContainText('Correct');
    // The proof is in the reading column now.
    await expect(screen.getByRole('region', 'Proof').getByRole('heading', 'src/drafts.rs new 7–8')).toBeVisible();
    // Why the agent thinks the item is at whiteboard level stays out of the page.
    await expect(screen.getByText('Data storage: what survives closing the pane.', { exact: false })).toHaveCount(0);

    await agent.act('go on to the next quiz question');
    await expect(screen.getByRole('region', 'Quiz · Question 2 of 2')).toBeVisible();
    await expect(screen.getByRole('region', 'Proof')).toHaveCount(0);
    await agent.act('answer the quiz question with {answer}, then check the answer', {
      params: { answer: SECOND_WRONG },
    });
    await expect(screen.getByRole('status')).toHaveText(
      'Not quite. The draft is a field of the round, saved wherever the round is saved.',
    );
    expect(await verdictInView()).toBe(true);
    // The wrong pick and the correct answer each say what they are.
    const answers = screen.getByRole('list', 'Answers').getByRole('listitem');
    const wrong = answers.filter({ hasText: 'Your pick' });
    await expect(wrong).toContainText(SECOND_WRONG);
    await expect(wrong).not.toContainText('Correct');
    await expect(answers.filter({ hasText: 'Correct' })).toContainText(SECOND_CORRECT);
    await expect(screen.getByRole('region', 'Proof')).toContainText('The round holds the draft.');
    // The dots beside the eyebrow say in words what their colours show.
    await expect(screen.getByRole('img', 'Question 1 correct, question 2 wrong')).toBeAttached();
    // The page still shows the item just checked: the rail names it, not the conclusion yet.
    await expect(rail.getByText('Quiz 2/2')).toHaveAttribute('aria-current', 'step');
    expect(await explore.quiz()).toEqual({
      picks: [
        { item: 0, answer: 1, correct: true },
        { item: 1, answer: 1, correct: false },
      ],
    });

    await agent.act('go on to the conclusion');
    await expect(screen.getByRole('region', 'Conclusion')).toBeVisible();
    await expect(rail.getByText('Conclusion')).toHaveAttribute('aria-current', 'step');
    // The rail's quiz step carries the score, and so does the panel. A phone's rail shows only
    // the design and the current step: there the panel's banner alone says it.
    if ((await rail.getByRole('listitem').count()) > 2) await expect(rail).toContainText('Quiz 1/2');
    await expect(screen.getByText('Quiz 1 of 2')).toBeVisible();
    await expect(screen.getByRole('heading', 'Quiz · 1 of 2 correct')).toBeVisible();

    // The page opened again shows the conclusion, with the results the round saved.
    await explore.open();
    await expect(screen.getByRole('heading', 'Quiz · 1 of 2 correct')).toBeVisible();
  },
);

test('the reviewer skips the quiz, reads the conclusion, then the answers', async ({ explore, screen, agent }) => {
  await explore.open();
  await explore.concludeWithQuiz();
  await expect(screen.getByRole('region', 'Quiz · Question 1 of 2')).toBeVisible();

  await agent.act('skip the quiz');
  await expect(screen.getByRole('region', 'Conclusion')).toBeVisible();
  await expect(screen.getByRole('heading', 'Quiz · 0 of 2 correct, 2 skipped')).toBeVisible();
  expect(await explore.quiz()).toEqual({ skipped: true });

  // The score opens the panel, with a way to the answers: each item shows its correct answer,
  // marked in words.
  await expect(screen.getByText('Quiz 0 of 2')).toBeVisible();
  await agent.act('see the answers of the quiz');
  const item = screen.getByRole('region', 'Question 1');
  await expect(item).toBeVisible();
  await expect(item).toContainText('Skipped.');
  await expect(item.getByRole('listitem').filter({ hasText: 'Correct' })).toContainText(FIRST_CORRECT);
});
