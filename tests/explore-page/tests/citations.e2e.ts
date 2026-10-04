// The code a question cites: the standalone server's first question cites lines of a changed
// Rust file after the change, then lines before it; its second question cites the whole file.
import { expect } from 'e2e';
import { test } from './session.ts';

test('a question shows the lines it cites, in the order the agent gave', async ({ explore, screen, agent }) => {
  await explore.open();
  await explore.askQuestion();
  await screen.getByRole('link', 'Go to question 1').tap();
  await expect(screen.getByRole('region', 'Question 1')).toBeVisible();

  await agent.act('show all the citations of question 1');
  const citations = screen.getByRole('region', 'Citations').getByRole('heading', { level: 4 });
  await expect(citations).toHaveText(['src/drafts.rs new 7–8', 'src/drafts.rs old 1–3']);
  // The first citation shows its note, and its lines as numbered rows of the diff: the removed
  // line right before the line that replaced it.
  const first = screen.getByRole('region', 'src/drafts.rs new 7–8');
  await expect(first).toContainText('this is the decision');
  // A phone shows one number column: the new line's, or the old one's on a removed row.
  await expect(first.getByRole('row')).toHaveText([/7 pub fn reopen/, /^7 − self\.draft/, /^8 \+ /]);
});

test('a citation of a whole file says so and shows no lines', async ({ explore, screen }) => {
  await explore.askQuestion();
  await explore.askQuestion();
  await explore.open();
  const citation = screen.getByRole('region', 'src/drafts.rs whole file');
  await expect(citation).toContainText('the citation names the whole file, not lines of it');
  await expect(citation.getByRole('row')).toHaveCount(0);
});

test('a citation of a file outside the change and the tracked files shows no lines', async ({ explore, screen }) => {
  await explore.open();
  await explore.askQuestion({
    id: 'secret-file',
    version: 1,
    topic: 'settings',
    text: 'Should the settings be read from the environment file?',
    rationale: null,
    visual: null,
    alternatives: [
      { id: 'read', text: 'Read the file', outcome: 'accepted', recommendation: null },
      { id: 'skip', text: 'Ignore the file', outcome: 'needs_follow_up', recommendation: null },
    ],
    evidence: [{ path: '.env', side: 'new', lines: { first_line: 1, last_line: 1 }, notes: 'The settings file.' }],
    assessments: null,
  });
  await screen.getByRole('link', 'Go to question 1').tap();
  const citation = screen.getByRole('region', '.env new 1');
  await expect(citation).toContainText("This file is not part of the repository's tracked files");
  await expect(citation.getByRole('row')).toHaveCount(0);
});
