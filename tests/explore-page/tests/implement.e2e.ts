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

  const tasks = screen.getByRole('textbox', 'To be implemented');
  const implement = screen.getByRole('button', 'Implement', { exact: true });
  // The tool refuses a blank list: Implement waits for a task.
  await tasks.fill(' \n\n ');
  await expect(implement).toBeDisabled();
  await tasks.fill(TASKS);
  await implement.tap();
  await expect(screen.getByRole('status')).toContainText('Sending the implementation request');
  expect(await explore.implementations()).toEqual([TASKS]);

  await explore.deliverImplementation();
  await expect(screen.getByRole('status')).toContainText('The agent received the implementation request');
  await expect(implement).toHaveCount(0);

  await screen.getByRole('button', 'Start a new round…').tap();
  await screen.getByRole('button', 'Confirm: start a new round').tap();
  await expect(screen.getByRole('button', 'Start', { exact: true })).toBeVisible();
  expect(await explore.actions()).toEqual(['reset']);
});

test('the reviewer cancels an implementation request that is being sent', async ({ explore, screen }) => {
  await explore.open();
  await explore.conclude();
  const implement = screen.getByRole('button', 'Implement', { exact: true });
  await implement.tap();
  await expect(screen.getByRole('status')).toContainText('Sending the implementation request');

  await screen.getByRole('button', 'Cancel the implementation request').tap();
  await expect(screen.getByRole('status')).toContainText('cancelled before it was sent');
  expect(await explore.actions()).toEqual(['cancel-implementation']);
  await expect(implement).toBeVisible();
});
