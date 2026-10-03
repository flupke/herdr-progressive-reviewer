// The code a question cites: the standalone server's first question cites lines of a changed
// Rust file after the change, then lines before it; its second question cites the whole file.
import { expect } from 'e2e';
import { test } from './session.ts';

test('a question shows the lines it cites, in the order the agent gave', async ({ explore, screen, agent }) => {
  await explore.open();
  await explore.askQuestion();
  await expect(screen.getByRole('region', 'Question 1')).toBeVisible();

  await agent.act('show all the citations of question 1');
  const citations = screen.getByRole('region', 'Citations').getByRole('heading', { level: 4 });
  await expect(citations).toHaveText(['src/drafts.rs new 7-8', 'src/drafts.rs old 1-3']);
  await agent.assert(
    'two citations are shown, each with a note and with lines of code as numbered rows of a diff; ' +
      'in the first citation (src/drafts.rs new 7-8), a removed line comes right before an added line',
  );
});

test('a citation of a whole file says so and shows no lines', async ({ explore, screen, agent }) => {
  await explore.askQuestion();
  await explore.askQuestion();
  await explore.open();
  await expect(screen.getByRole('region', 'Question 2')).toBeVisible();

  await agent.assert('question 2 cites the whole file src/drafts.rs, shows no lines of code for it, and says why');
});
