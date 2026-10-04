import { expect } from 'e2e';
import { test } from './session.ts';

// The reviewer controls the whole round from the page, as from the reviewer's Explore tab: Stop
// waiting, Retry, Cancel answer, Reset, a reply to the conclusion, and the cancel of an
// implementation request. The fixture's round starts with the agent working on its first turn,
// the kickoff; `explore.actions()` lists the actions the page sent, as the session took them.

// What a Retry achieves on the page: the agent works on the turn again.
const RETRYING = {
  agentContext: 'Retry on this page is done once the page says that the agent is working.',
};

test('the reviewer recovers a kickoff that could not be delivered with Retry', RETRYING, async ({ explore, screen, agent }) => {
  await explore.open();
  await explore.failDelivery();
  await expect(screen.getByRole('alert')).toContainText('The selected agent is no longer available');

  await agent.act('send the turn to the agent again with Retry');
  await expect(screen.getByRole('status')).toContainText('The agent is working');
  expect(await explore.actions()).toEqual(['retry']);

  await explore.askQuestion();
  await expect(screen.getByRole('region', 'Question 1')).toBeVisible();
});

test('the reviewer retries a turn the agent did not start on', RETRYING, async ({ explore, screen, agent }) => {
  await explore.open();
  await explore.agentDoesNotStart();
  await expect(screen.getByRole('alert')).toContainText("The agent did not start on the turn's prompt");

  await agent.act('send the turn to the agent again with Retry');
  await expect(screen.getByRole('status')).toContainText('The agent is working');
  expect(await explore.actions()).toEqual(['retry']);
});

// What Stop waiting, then Retry, achieve on the page: the turn is paused, then the agent works on
// it again.
const STOPPING = {
  agentContext:
    'Stop waiting on this page is done once the page says that the turn is paused. Retry is done once the page says that the agent is working.',
};

test('the reviewer stops waiting for the agent, then retries', STOPPING, async ({ explore, screen, agent }) => {
  await explore.open();
  await expect(screen.getByRole('status')).toContainText('The agent is working');

  await agent.act('stop waiting for the agent');
  await expect(screen.getByRole('status')).toContainText('The turn is paused');
  expect(await explore.actions()).toEqual(['stop']);

  await agent.act('send the turn to the agent again with Retry');
  await expect(screen.getByRole('status')).toContainText('The agent is working');
  expect(await explore.actions()).toEqual(['stop', 'retry']);
});

test(
  'the reviewer cancels the last answer, and its question waits again',
  {
    agentContext:
      'Cancel answer on this page is done once the question waits for an answer again, with Send.',
  },
  async ({ explore, screen, agent }) => {
  await explore.open();
  await explore.askQuestion();
  await explore.answerInPane();
  await expect(screen.getByRole('region', 'Your last answer')).toContainText('Keep the draft');

  await agent.act('cancel your last answer');
  await expect(screen.getByRole('region', 'Question 1')).toBeVisible();
  await expect(screen.getByRole('button', 'Send')).toBeVisible();
  expect(await explore.actions()).toEqual(['cancel-answer']);
  await expect(screen.getByRole('region', 'Your last answer')).toHaveCount(0);
});

test(
  'the reviewer resets the round, then starts a new one',
  {
    agentContext:
      'Starting an Explore round on this page is done once the page says that it is preparing the round.',
  },
  async ({ explore, screen, agent }) => {
    await explore.open();
    await explore.askQuestion();
    await expect(screen.getByRole('region', 'Question 1')).toBeVisible();

    await agent.act('reset the round from the menu, and confirm the reset');
    await expect(screen.getByRole('status')).toContainText('No round is running', { ignoreCase: true });
    expect(await explore.actions()).toEqual(['reset']);

    await agent.act('start an Explore round without the Challenger');
    await expect(screen.getByRole('status')).toContainText('Preparing the round');
    expect(await explore.starts()).toEqual([{ challenger: false }]);
  },
);

// What a reply achieves: the agent takes it up afterwards, out of the page's hands.
const REPLYING = {
  agentContext: 'Replying to the conclusion on this page is done once the page says that the agent is working.',
};

test('the reviewer replies to the conclusion', REPLYING, async ({ explore, screen, agent }) => {
  await explore.open();
  await explore.conclude();
  await expect(screen.getByRole('region', 'Conclusion')).toBeVisible();

  await agent.act('reply {reply} to the conclusion', { params: { reply: 'Why not keep the draft in memory?' } });
  await expect(screen.getByRole('status')).toContainText('The agent is working');
  expect(await explore.actions()).toEqual(['reply']);
});

test('the reviewer cancels an implementation request that is being sent', async ({ explore, screen, agent }) => {
  await explore.open();
  await explore.conclude();
  // An exact action: the setup is a request on its way.
  await screen.getByRole('button', 'Implement 1 item').tap();
  await expect(screen.getByRole('status')).toContainText('Sending the implementation request');

  await agent.act('cancel the implementation request');
  await expect(screen.getByRole('status')).toContainText('cancelled before it was sent');
  expect(await explore.actions()).toEqual(['cancel-implementation']);
  await expect(screen.getByRole('button', 'Implement 1 item')).toBeVisible();
});

test('a Retry of a turn the round moved past meanwhile is refused with a notice that names it', async ({ explore, screen }) => {
  await explore.open();
  await explore.failDelivery();
  await expect(screen.getByRole('button', 'Retry')).toBeVisible();
  await explore.holdPage();
  await explore.answerInPane();

  // An exact action: the refusal of this Retry is the point of the test, which a goal to retry
  // would count as a failure.
  await screen.getByRole('button', 'Retry').tap();
  await expect(screen.getByRole('alert')).toContainText('Retry did nothing');
  await expect(screen.getByRole('status')).toContainText('The agent is working');
  expect(await explore.actions()).toEqual([]);
});

test('a comment being typed comes back when the page follows the round back to its question', async ({
  explore,
  screen,
}) => {
  await explore.open();
  await explore.askQuestion();
  // Exact actions: the comment must receive this exact text, then the pane moves the round away
  // from the question and back.
  await screen.getByRole('textbox', 'Comment (optional)').fill('Keep it, but log the overflow.');
  await explore.answerInPane();
  await expect(screen.getByRole('status')).toContainText('The agent is working');

  await explore.cancelAnswerInPane();
  await expect(screen.getByRole('region', 'Question 1')).toBeVisible();
  await expect(screen.getByRole('textbox', 'Comment (optional)')).toHaveValue('Keep it, but log the overflow.');
});
