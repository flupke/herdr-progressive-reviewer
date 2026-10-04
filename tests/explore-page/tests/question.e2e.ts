import { expect } from 'e2e';
import { test } from './session.ts';

test('the page shows the question the agent posts, and the reviewer picks a choice', async ({
  explore,
  screen,
  agent,
}) => {
  // The page opens while the agent works, and follows the round when the question comes.
  await explore.open();
  await expect(screen.getByRole('status')).toContainText('The agent is working');
  await explore.askQuestion();
  await screen.getByRole('link', 'Go to question 1').tap();
  const question = screen.getByRole('region', 'Question 1');
  await expect(question).toContainText("Should a reopened round keep the reviewer's unsent draft?");
  await expect(screen.getByRole('status')).not.toBeVisible();
  // Beside the agent's choices, which the steps below pick, the page offers None of the above.
  await expect(question.getByRole('radio', 'None of the above')).toBeVisible();

  await agent.act('pick "Discard the draft" as the answer to question 1');
  await expect(screen.getByRole('radio', 'Discard the draft')).toBeChecked();
});

test('the question shows as Markdown, with raw HTML as text', async ({ explore, screen }) => {
  await explore.open();
  await explore.askQuestion({
    id: 'reopen-draft',
    version: 1,
    topic: 'drafts',
    text: 'Should `reopen()` keep the **unsent** draft? <b>Yes</b>',
    rationale: null,
    visual: null,
    alternatives: [
      { id: 'keep', text: 'Keep the draft', outcome: 'accepted', recommendation: null },
      { id: 'discard', text: 'Discard the draft', outcome: 'needs_follow_up', recommendation: null },
    ],
    evidence: [],
    assessments: null,
  });
  await screen.getByRole('link', 'Go to question 1').tap();
  const question = screen.getByRole('region', 'Question 1');
  await expect(question).toContainText('Should reopen() keep the unsent draft? <b>Yes</b>');
  await expect(question.getByText('reopen()', { exact: true })).toBeVisible();
  await expect(question.getByText('unsent', { exact: true })).toBeVisible();
  await expect(question).not.toContainText('`');
});

// A choice, its reason and the answer kept are the agent's plain text: only their code spans
// show as code, wherever the page shows them.
const CODE_IN_CHOICES = {
  id: 'draft-home',
  version: 1,
  topic: 'drafts',
  text: 'Where should a reopened round find its draft?',
  rationale: null,
  visual: null,
  alternatives: [
    {
      id: 'round',
      text: 'Keep the draft in `Round::draft`',
      outcome: 'accepted',
      recommendation: 'Then `reopen()` finds the draft without a second store.',
    },
    { id: 'editor', text: 'Keep the draft in the editor', outcome: 'needs_follow_up', recommendation: null },
  ],
  evidence: [],
  assessments: null,
};

test("a choice's code shows as code on the question, in the previous turn and in the decisions", async ({
  explore,
  screen,
}) => {
  await explore.open();
  await explore.askQuestion(CODE_IN_CHOICES);
  await screen.getByRole('link', 'Go to question 1').tap();
  const question = screen.getByRole('region', 'Question 1');
  // The backticks are gone from the choice's name, and the code is an element of its own.
  await expect(question.getByRole('radio', 'Keep the draft in Round::draft')).toBeVisible();
  await expect(question.getByText('Round::draft', { exact: true })).toBeVisible();
  await expect(question.getByText('reopen()', { exact: true })).toBeVisible();
  await expect(question).not.toContainText('`');

  // The reviewer keeps that choice in the pane; the next question recalls it.
  await explore.answerInPane();
  await explore.askQuestion();
  const answered = screen.getByRole('region', 'You answered Q1');
  await expect(answered.getByText('Round::draft', { exact: true })).toBeVisible();
  await expect(answered).not.toContainText('`');

  await explore.answerInPane();
  await explore.conclude();
  const decisions = screen.getByRole('region', 'Your decisions');
  await expect(decisions.getByText('Round::draft', { exact: true })).toBeVisible();
  await expect(decisions).not.toContainText('`');
});
