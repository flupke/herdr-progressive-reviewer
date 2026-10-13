// The glossary: the terms the agent defined are marked wherever the page shows them, a tap shows
// a term's definition, and the design ends on the list of terms.
import { expect } from 'e2e';
import { test } from './session.ts';

test('a marked term shows its definition, and the design lists every term', async ({ explore, screen }) => {
  await explore.open();
  await explore.askQuestion();
  await screen.getByRole('link', 'Go to question 1').tap();
  const question = screen.getByRole('region', 'Question 1');

  // "unsent draft" in the question: the term is marked, inside the headline's prose.
  await question.getByRole('button', /^draft$/).first().tap();
  // The definition alone: the term is the one tapped.
  await expect(screen.getByRole('tooltip')).toHaveText(
    'The text the reviewer typed in the answer box and has not sent yet.',
  );
  // A tap inside a choice's label opens the definition, not the choice.
  await expect(question.getByRole('radio', 'Keep the draft')).not.toBeChecked();

  await explore.openDesign();
  const glossary = screen.getByRole('region', 'Glossary');
  await expect(glossary).toContainText('reopen()');
  await expect(glossary).toContainText('The text the reviewer typed in the answer box and has not sent yet.');
});
