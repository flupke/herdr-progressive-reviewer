import { expect } from 'e2e';
import { test } from './session.ts';

// The standalone server's round opens with the design of the change, which its first turn
// explained with a table and a callout.
test('the round opens with the design of the change, which the reviewer opens again later', async ({
  explore,
  screen,
  agent,
}) => {
  await explore.open();
  await explore.askQuestion();
  await expect(screen.getByRole('region', 'Question 1')).toBeVisible();
  // At the first question, the design shows open, its Markdown drawn: the table of its data flow.
  await expect(screen.getByRole('region', 'Design of the change').getByRole('table')).toBeVisible();

  // Later in the round, the design is folded away above the question.
  await explore.answerInPane();
  await explore.askQuestion();
  await explore.open();
  await expect(screen.getByRole('region', 'Question 2')).toBeVisible();
  const design = screen.getByRole('heading', 'Types and data flow');
  await expect(design).toBeHidden();
  await agent.act('open the design of the change');
  await expect(design).toBeVisible();
});
