// A question the agent revises from the round's chat: the new version takes the screen, says
// why, and keeps the comment the reviewer was typing.
import { expect } from 'e2e';
import { test } from './session.ts';

const COMMENT = 'Only while the round lives.';

/** The short data set's first question, as the agent asks it again after a message. */
const REVISED = {
  id: 'keep-draft',
  version: 2,
  topic: 'drafts',
  text: 'Should a reopened round keep one draft per question?',
  rationale: null,
  visual: null,
  alternatives: [
    { id: 'keep', text: 'Keep one draft per question', outcome: 'accepted', recommendation: null },
    { id: 'discard', text: 'Discard every draft', outcome: 'needs_follow_up', recommendation: null },
  ],
  evidence: [],
  assessments: null,
};

test('a revised question replaces the open one, says why, keeps the comment and drops the pick', async ({
  explore,
  screen,
}) => {
  await explore.open();
  await explore.askQuestion();
  await screen.getByRole('link', 'Go to question 1').tap();
  const question = screen.getByRole('region', 'Question 1');
  await question.getByRole('radio', 'Keep the draft').check();
  await question.getByRole('textbox', 'Comment · optional').fill(COMMENT);

  await explore.reviseQuestion(REVISED);
  await expect(question).toContainText('Should a reopened round keep one draft per question?');
  // The reviewer was typing: the comment box keeps the focus.
  await expect(question.getByRole('textbox', 'Comment · optional')).toBeFocused();
  await expect(question.getByRole('radio', 'Keep one draft per question')).not.toBeChecked();
  // A second revision keeps the comment too.
  await explore.reviseQuestion({ ...REVISED, version: 3 });
  await expect(question.getByRole('textbox', 'Comment · optional')).toHaveValue(COMMENT);

  await expect(question.getByRole('note')).toContainText('The agent revised this question after your message');
  await expect(question.getByRole('note')).toContainText('one draft per question');
  await expect(question.getByRole('textbox', 'Comment · optional')).toHaveValue(COMMENT);
  await question.getByRole('radio', 'Keep one draft per question').check();
  await question.getByRole('button', 'Send answer').tap();
  await expect(screen.getByRole('status')).toContainText('The agent is working on your answer to question 1');
  expect(await explore.answers()).toEqual([{ question: 'keep-draft', version: 3, choice: 'keep', comment: COMMENT }]);
});
