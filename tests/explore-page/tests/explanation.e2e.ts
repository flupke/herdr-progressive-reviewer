import { expect } from 'e2e';
import { test } from './session.ts';

// The standalone server's first question explains itself with a table whose cells carry status
// marks, a Tip callout, raw HTML, and Door and Blast radius sections folded to their leads.
test('a question shows its explanation, the lead of Blast radius, and the rest on request', async ({
  explore,
  screen,
  agent,
}) => {
  await explore.open();
  await explore.askQuestion();
  await screen.getByRole('link', 'Go to question 1').tap();
  const question = screen.getByRole('region', 'Question 1');
  // A status mark in a cell of the table, and the callout.
  await expect(question.getByRole('cell').getByRole('image', 'Warning')).toBeVisible();
  await expect(question.getByLabel('Tip')).toBeVisible();
  // The agent's raw HTML shows as the text it typed.
  await expect(screen.getByText('A draft keeps what the reviewer typed, such as <b>tags</b>, as plain text.')).toBeVisible();

  // Folded, a section shows its lead: the decisive reason.
  await expect(question.getByText('Two-way — Removing the field later drops only unsent drafts.')).toBeVisible();
  await expect(question).toContainText('Two-way door');
  const unknown = screen.getByText('Unknown: How long drafts grow in long rounds.');
  await expect(unknown).toBeHidden();
  await agent.act('open the "Blast radius" section of question 1');
  await expect(unknown).toBeVisible();
});
