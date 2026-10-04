import { expect } from 'e2e';
import { test } from './session.ts';

// The masthead above every screen: the round rail says where the round stands, "Design ▾" opens
// the design map, and the browser tab's title says whose turn it is. Reset, in the masthead's
// menu, is checked in control.e2e.ts. On a phone the rail shows the design and the current step
// as two chips: the same checks hold there.

/** The review of the fixture's session, which the tab title names. */
const REVIEW = "Keep the reviewer's draft when a round reopens";

test('the rail and the tab title follow the round through an answer', async ({ explore, screen, agent, browser }) => {
  await explore.open();
  await explore.askQuestion();
  const rail = screen.getByRole('navigation', 'Round');
  await expect(rail.getByText('Q1')).toHaveAttribute('aria-current', 'step');
  await expect(rail.getByRole('button', 'Design')).toBeVisible();
  await expect(browser).toHaveTitle(`Q1 · your turn — ${REVIEW}`);

  await agent.act('answer question 1 with "Discard the draft" and send it');
  await expect(rail.getByText('Q1 · working')).toHaveAttribute('aria-current', 'step');
  await expect(browser).toHaveTitle(`Agent working… — ${REVIEW}`);

  await explore.askQuestion();
  await expect(rail.getByText('Q2')).toHaveAttribute('aria-current', 'step');
  await expect(browser).toHaveTitle(`Q2 · your turn — ${REVIEW}`);
});

test('the design map on the rail leads to a part of the design', async ({ explore, screen, agent }) => {
  await explore.open();
  await explore.askQuestion();
  await explore.answerInPane();
  await explore.askQuestion();
  // From question 2 on, the design is folded away under the masthead.
  const design = screen.getByRole('region', 'Design of the change');
  await expect(design.getByRole('heading', 'Types and data flow')).toBeHidden();

  await agent.act('open the design map from the round rail, and go to its part "Types and data flow"');
  await expect(design.getByRole('heading', 'Types and data flow')).toBeVisible();
});
