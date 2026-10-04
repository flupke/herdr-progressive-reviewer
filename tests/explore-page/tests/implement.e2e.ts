import { expect } from 'e2e';
import { test } from './session.ts';

const TASKS = 'Save the draft with the round, and test that a reopened round restores it.';

// Pressing Implement hands the request to the reviewer, which sends it to the agent; the page
// says so until the test lets the agent receive it.
const SENDING =
  'Once Implement is pressed, the page says that it is sending the implementation request to the ' +
  'agent. That is the expected result of pressing Implement, not a failure: do not wait for the ' +
  'agent to receive it.';

test('the reviewer edits the list to be implemented, sends it, then sees it sent', { agentContext: SENDING }, async ({
  explore,
  screen,
  agent,
}) => {
  await explore.open();
  await explore.conclude();
  await expect(screen.getByRole('region', 'Conclusion')).toBeVisible();

  await agent.act('replace the list to be implemented with {tasks}, then press Implement', {
    params: { tasks: TASKS },
  });
  await expect(screen.getByRole('status')).toContainText('Sending the implementation request');
  expect(await explore.implementations()).toEqual([TASKS]);

  await explore.deliverImplementation();
  await expect(screen.getByRole('status')).toContainText('sent to the agent');
  await expect(screen.getByText(TASKS)).toBeVisible();
  await expect(screen.getByRole('button', 'Implement')).toHaveCount(0);
});

test('a list of several lines is sent with the line breaks a list written in the pane has', async ({
  explore,
  screen,
}) => {
  const tasks = 'Save the draft with the round.\nTest that a reopened round restores it.';
  await explore.open();
  await explore.conclude();

  // Exact actions: the list must receive this exact text, line breaks included.
  await screen.getByRole('textbox', 'To be implemented').fill(tasks);
  await screen.getByRole('button', 'Implement').tap();
  await expect(screen.getByRole('status')).toContainText('Sending the implementation request');
  expect(await explore.implementations()).toEqual([tasks]);
});

test('an Implement after the pane sent the request is refused', async ({ explore, screen }) => {
  await explore.open();
  await explore.conclude();
  // The page shows the conclusion, and does not follow the request sent from the pane.
  await expect(screen.getByRole('region', 'Conclusion')).toBeVisible();
  await explore.implementInPane();

  // An exact action: the refusal of this Implement is the point of the test, which a goal to
  // implement would count as a failure.
  await screen.getByRole('button', 'Implement').tap();
  await expect(screen.getByRole('alert')).toContainText('The implementation request was not sent');
  await expect(screen.getByRole('status')).toContainText('sent to the agent');
  expect(await explore.implementations()).toEqual([]);
});

test(
  'a request that could not be sent shows why, and keeps the edited list to send again',
  { agentContext: SENDING },
  async ({ explore, screen, agent }) => {
    await explore.open();
    await explore.conclude();
    await expect(screen.getByRole('region', 'Conclusion')).toBeVisible();
    await agent.act('replace the list to be implemented with {tasks}, then press Implement', {
      params: { tasks: TASKS },
    });
    await expect(screen.getByRole('status')).toContainText('Sending the implementation request');

    await explore.failDelivery();
    await expect(screen.getByRole('alert')).toContainText('The selected agent is no longer available');
    await expect(screen.getByRole('textbox', 'To be implemented')).toHaveValue(TASKS);

    await agent.act('send the list to be implemented again with Implement');
    await expect(screen.getByRole('status')).toContainText('Sending the implementation request');
    expect(await explore.implementations()).toEqual([TASKS, TASKS]);
  },
);

test('a request the agent did not start on says so, and leaves Retry to the pane', { agentContext: SENDING }, async ({
  explore,
  screen,
  agent,
}) => {
  await explore.open();
  await explore.conclude();
  await expect(screen.getByRole('region', 'Conclusion')).toBeVisible();
  await agent.act('replace the list to be implemented with {tasks}, then press Implement', {
    params: { tasks: TASKS },
  });
  await expect(screen.getByRole('status')).toContainText('Sending the implementation request');

  await explore.agentDoesNotStart();
  // The text may still wait in the agent's prompt box: the page sends no second request.
  await expect(screen.getByRole('alert')).toContainText('did not start on the implementation request');
  await expect(screen.getByRole('button', 'Implement')).toHaveCount(0);
  await expect(screen.getByText(TASKS)).toBeVisible();
});
