import { expect } from 'e2e';
import { test } from './session.ts';

// The design of the change is a screen of its own: it opens the round, and the reviewer opens it
// again from any later stage. The standalone server's design has four parts, the second with a
// sequence diagram and a table, in a change of one file.

const THESIS =
  "Reopening a round brings back the reviewer's unsent draft, saved with the editor state in the round record.";

test('the round opens on the design of the change, then the reviewer goes on to question 1', async ({
  explore,
  screen,
  agent,
}) => {
  await explore.open();
  await explore.askQuestion();
  const design = screen.getByRole('region', 'Design of the change');
  // The change's thesis is the headline, with how long the design is to read.
  await expect(design.getByRole('heading', { level: 2 })).toHaveText(THESIS);
  await expect(design.getByText('4 parts · about 1 minute · 1 file')).toBeVisible();
  // Each part leads with its thesis, then its diagram and its table.
  const dataFlow = design.getByRole('region', 'Types and data flow');
  await expect(dataFlow.getByText('The draft is saved with the editor state, in the round record.')).toBeVisible();
  await expect(dataFlow.getByRole('table')).toBeVisible();
  await expect(screen.getByRole('region', 'Question 1')).toBeHidden();
  // The rail shows the design as the step in view, the round's own step next.
  const rail = screen.getByRole('navigation', 'Round');
  await expect(rail.getByRole('listitem').first()).toHaveAttribute('aria-current', 'page');
  await expect(rail.getByText('Q1')).toHaveAttribute('aria-current', 'step');

  await agent.act('go on from the design to question 1');
  await expect(screen.getByRole('region', 'Question 1')).toBeVisible();
  await expect(design).toBeHidden();
});

test('the design map marks the part the reviewer reads', async ({ explore, screen, agent }) => {
  await explore.open();
  await explore.askQuestion();
  const map = screen.getByRole('navigation', 'Design map');
  await expect(map.getByRole('link', 'What it adds and where')).toHaveAttribute('aria-current', 'location');

  await agent.act('open "Rejected alternatives" from the design map');
  await expect(map.getByRole('link', 'Rejected alternatives')).toHaveAttribute('aria-current', 'location');
  await expect(map.getByRole('link', 'What it adds and where')).not.toHaveAttribute('aria-current', 'location');
});

test('the reviewer opens the design again from a later question, then goes back to it', async ({
  explore,
  screen,
  agent,
}) => {
  await explore.askQuestion();
  await explore.answerInPane();
  await explore.askQuestion();
  await explore.open();
  // Past question 1, the page shows the round's current stage.
  await expect(screen.getByRole('region', 'Question 2')).toBeVisible();

  // The address the design map of the rail's "Design ▾" links to.
  await explore.openDesign();
  await expect(screen.getByRole('region', 'Design of the change')).toBeVisible();
  await expect(screen.getByRole('region', 'Question 2')).toBeHidden();

  await agent.act('go back to question 2');
  await expect(screen.getByRole('region', 'Question 2')).toBeVisible();
  await expect(screen.getByRole('region', 'Design of the change')).toBeHidden();
});

test('the design that opened the round gives way to the round once it moves on in the pane', async ({
  explore,
  screen,
}) => {
  await explore.open();
  await explore.askQuestion();
  await expect(screen.getByRole('region', 'Design of the change')).toBeVisible();

  await explore.answerInPane();
  await expect(screen.getByRole('status')).toContainText('The agent is working');
  await expect(screen.getByRole('region', 'Design of the change')).toBeHidden();
});
