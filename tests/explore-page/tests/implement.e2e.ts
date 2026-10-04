import { expect } from 'e2e';
import { test } from './session.ts';

const TASKS = 'Save the draft with the round, and test that a reopened round restores it.';

// Implement counts the items of the list: the fixed conclusion's list has one.
const IMPLEMENT = 'Implement 1 item';

// Sending a request (Implement, Retry, Send the saved request, Send a new request anyway) hands
// it to the reviewer, which sends it to the agent; the page says so until the test lets the agent
// receive it.
const SENDING =
  'Once an implementation request is sent, the page says that it is sending the implementation ' +
  'request to the agent. That is the expected result of sending it, not a failure: do not wait for ' +
  'the agent to receive it.';

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
  await expect(screen.getByRole('status')).toContainText('The agent received the implementation request');
  await expect(screen.getByText(TASKS)).toBeVisible();
  await expect(screen.getByRole('button', IMPLEMENT)).toHaveCount(0);
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
  // Implement counts the items as the reviewer types them.
  await screen.getByRole('button', 'Implement 2 items').tap();
  await expect(screen.getByRole('status')).toContainText('Sending the implementation request');
  expect(await explore.implementations()).toEqual([tasks]);
});

test('an Implement after the pane sent the request is refused', async ({ explore, screen }) => {
  await explore.open();
  await explore.conclude();
  // The page shows the conclusion, and is held there while the pane sends the request.
  await expect(screen.getByRole('region', 'Conclusion')).toBeVisible();
  await explore.holdPage();
  await explore.implementInPane();

  // An exact action: the refusal of this Implement is the point of the test, which a goal to
  // implement would count as a failure.
  await screen.getByRole('button', IMPLEMENT).tap();
  await expect(screen.getByRole('alert')).toContainText('This conclusion no longer waits for a request');
  await expect(screen.getByRole('status')).toContainText('The agent received the implementation request');
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

test('a request the agent did not start on says so, and the reviewer sends it again with Retry', { agentContext: SENDING }, async ({
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
  // The text may still wait in the agent's prompt box: the page offers no new request, only
  // Retry of the same one.
  await expect(screen.getByRole('alert')).toContainText('did not start on the implementation request');
  await expect(screen.getByRole('button', IMPLEMENT)).toHaveCount(0);
  await expect(screen.getByText(TASKS)).toBeVisible();

  await agent.act('send the implementation request again with Retry');
  await expect(screen.getByRole('status')).toContainText('Sending the implementation request');
  expect(await explore.actions()).toEqual(['resend-implementation']);
  expect(await explore.implementations()).toEqual([TASKS]);
});

test('a request the review never sent waits, and the reviewer sends it as it was saved', { agentContext: SENDING }, async ({
  explore,
  screen,
  agent,
}) => {
  await explore.open();
  await explore.conclude();
  // Exact actions: the setup is a request saved, then left behind by a reopened review.
  await screen.getByRole('button', IMPLEMENT).tap();
  await expect(screen.getByRole('status')).toContainText('Sending the implementation request');
  await explore.reopenBeforeSending();
  await expect(screen.getByRole('status')).toContainText('The request was never sent');

  await agent.act('send the saved implementation request');
  await expect(screen.getByRole('status')).toContainText('Sending the implementation request');
  expect(await explore.actions()).toEqual(['resend-implementation']);
  expect(await explore.implementations()).toEqual(['Save the draft with the round.']);
});

test('a request the agent may have received is sent again only on purpose', { agentContext: SENDING }, async ({
  explore,
  screen,
  agent,
}) => {
  await explore.open();
  await explore.conclude();
  // Exact actions: the setup is a request whose delivery a reopened review left unknown.
  await screen.getByRole('button', IMPLEMENT).tap();
  await expect(screen.getByRole('status')).toContainText('Sending the implementation request');
  await explore.reopenWhileSending();
  await expect(screen.getByRole('status')).toContainText('The agent may or may not have the request');
  // The risky action is the only one, and not the primary one: no Implement.
  await expect(screen.getByRole('button', IMPLEMENT)).toHaveCount(0);

  await agent.act('send a new implementation request anyway');
  await expect(screen.getByRole('status')).toContainText('Sending the implementation request');
  expect(await explore.implementations()).toEqual(['Save the draft with the round.', 'Save the draft with the round.']);
});

// Reset ends the round: the page then offers to start the next one, which is not part of the goal.
const RESET = {
  agentContext:
    'Starting a new round from the panel is done once the page says that no round is running and offers ' +
    'Start: do not press Start.',
};

test('once the agent received the request, the reviewer starts a new round', RESET, async ({
  explore,
  screen,
  agent,
}) => {
  await explore.open();
  await explore.conclude();
  // Exact actions: the setup is a request the agent received.
  await screen.getByRole('button', IMPLEMENT).tap();
  await explore.deliverImplementation();
  await expect(screen.getByRole('status')).toContainText('The agent received the implementation request');

  await agent.act('start a new round from the panel, and confirm it');
  await expect(screen.getByRole('button', 'Start', { exact: true })).toBeVisible();
  expect(await explore.actions()).toEqual(['reset']);
});

/** A two-way question whose first choice, the one an answer in the pane keeps, the agent
 * recommends. */
const RECOMMENDED_FIRST = {
  id: 'same-name-files',
  version: 1,
  topic: 'export',
  text: 'What should the export do with two files of the same name?',
  rationale: null,
  visual: null,
  alternatives: [
    { id: 'rename', text: 'Rename the clashing files', outcome: 'accepted', recommendation: 'Nothing is lost.' },
    { id: 'overwrite', text: 'Overwrite the older file', outcome: 'needs_follow_up', recommendation: null },
  ],
  evidence: [],
  assessments: null,
};

test("the conclusion lists the reviewer's decisions, with the choice kept and its tags", async ({ explore, screen }) => {
  await explore.open();
  await explore.askQuestion(RECOMMENDED_FIRST);
  await explore.answerInPane();
  // The fixed question recommends no choice: the reviewer keeps one after a first pick of the
  // other.
  await explore.askQuestion();
  await explore.answerAfterFirstPick();
  await explore.askQuestion({ ...RECOMMENDED_FIRST, id: 'same-name-folders' });
  await explore.answerAfterFirstPick();
  await explore.conclude();

  const decisions = screen.getByRole('region', 'Your decisions').getByRole('listitem');
  await expect(decisions).toHaveCount(3);
  await expect(decisions.nth(0)).toContainText('Rename the clashing files as recommended');
  await expect(decisions.nth(1)).toContainText('In the round\'s record changed after your first pick');
  await expect(decisions.nth(1)).not.toContainText('as recommended');
  await expect(decisions.nth(2)).toContainText('Rename the clashing files changed after your first pick as recommended');
});
