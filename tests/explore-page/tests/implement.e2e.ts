// The conclusion's implementation request: the reviewer edits the list, sends it with
// Implement, and may cancel it while it is on its way.
import { expect } from 'e2e';
import { test } from './session.ts';

const TASKS = 'Save the draft with the round.\nTest that a reopened round restores it.';

test('the reviewer edits the list, sends it, sees it received, then starts a new round', async ({
  explore,
  screen,
}) => {
  await explore.open();
  await explore.conclude();

  await screen.getByRole('textbox', 'To be implemented').fill(TASKS);
  // Implement counts the items as the reviewer types them.
  await screen.getByRole('button', 'Implement 2 items').tap();
  await expect(screen.getByRole('status')).toContainText('Sending the implementation request');
  expect(await explore.implementations()).toEqual([TASKS]);

  await explore.deliverImplementation();
  await expect(screen.getByRole('status')).toContainText('The agent received the implementation request');
  await expect(screen.getByRole('button', 'Implement 2 items')).toHaveCount(0);

  await screen.getByRole('button', 'Start a new round…').tap();
  await screen.getByRole('button', 'Confirm: start a new round').tap();
  await expect(screen.getByRole('button', 'Start', { exact: true })).toBeVisible();
  expect(await explore.actions()).toEqual(['reset']);
});

test('the reviewer cancels an implementation request that is being sent', async ({ explore, screen }) => {
  await explore.open();
  await explore.conclude();
  await screen.getByRole('button', 'Implement 1 item').tap();
  await expect(screen.getByRole('status')).toContainText('Sending the implementation request');

  await screen.getByRole('button', 'Cancel the implementation request').tap();
  await expect(screen.getByRole('status')).toContainText('cancelled before it was sent');
  expect(await explore.actions()).toEqual(['cancel-implementation']);
  await expect(screen.getByRole('button', 'Implement 1 item')).toBeVisible();
});
