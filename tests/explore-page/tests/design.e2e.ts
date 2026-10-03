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
  // The page loads the question once its script sees it; a judgement does not wait.
  await expect(screen.getByRole('region', 'Question 1')).toBeVisible();
  await agent.assert(
    'before question 1, the page explains the design of the change: what it adds and where, its ' +
      'types and data flow, its algorithm and cost, and the alternatives that were rejected; the ' +
      'explanation includes a table and a callout',
  );

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
