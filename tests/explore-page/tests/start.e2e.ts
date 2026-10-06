import { expect } from 'e2e';
import { test } from './session.ts';

// The fixture's round starts with the agent working: each test resets it first, so that the page
// shows that no round is running and offers to start one.

const START = 'start an Explore round without the Challenger';

// What a start on the page achieves: the review tool starts the round afterwards, out of the
// page's hands, and the fixture plays that part.
const STARTING = {
  agentContext:
    'Starting an Explore round on this page is done once the page says that it is preparing the round.',
};

test('the reviewer starts a round on the page, then sees it prepared and its first question', STARTING, async ({
  explore,
  screen,
  agent,
}) => {
  await explore.open();
  await explore.reset();
  await expect(screen.getByRole('button', 'Start')).toBeVisible();

  await agent.act(START);
  await expect(screen.getByRole('status')).toContainText('Preparing the round');
  expect(await explore.starts()).toEqual([{ challenger: false }]);

  await explore.sendKickoff();
  await expect(screen.getByRole('status')).toContainText('The agent is working');
  await explore.askQuestion();
  // The round opens on its design, which leads to question 1.
  await expect(screen.getByRole('region', 'Design of the change')).toBeVisible();
});

test('the reviewer starts a round with the Challenger', STARTING, async ({ explore, screen, agent }) => {
  await explore.open();
  await explore.reset();
  await expect(screen.getByRole('button', 'Start with Challenger')).toBeVisible();

  await agent.act('start an Explore round with the Challenger');
  await expect(screen.getByRole('status')).toContainText('Preparing the round');
  expect(await explore.starts()).toEqual([{ challenger: true }]);
});

test('a start that fails says why, and the reviewer can start again', STARTING, async ({ explore, screen, agent }) => {
  await explore.open();
  await explore.reset();
  await expect(screen.getByRole('button', 'Start')).toBeVisible();
  await agent.act(START);
  await expect(screen.getByRole('status')).toContainText('Preparing the round');

  await explore.failStart();
  await expect(screen.getByRole('alert')).toContainText('Repository comparison is not ready');
  await expect(screen.getByRole('button', 'Start')).toBeVisible();
});

test('a start after a round started in the reviewer meanwhile is refused', async ({ explore, screen }) => {
  await explore.open();
  await explore.reset();
  // The page shows that no round is running, and is held there while a round starts in the pane.
  await expect(screen.getByRole('button', 'Start')).toBeVisible();
  await explore.holdPage();
  await explore.sendKickoff();

  // An exact action: the refusal of this start is the point of the test, which a goal to start
  // a round would count as a failure.
  await screen.getByRole('button', 'Start').tap();
  await expect(screen.getByRole('alert')).toContainText('A round was started meanwhile');
  await expect(screen.getByRole('status')).toContainText('The agent is working');
  expect(await explore.starts()).toEqual([]);
});

test('the start screen names the review and the repository the page belongs to', async ({ explore, screen }) => {
  await explore.open();
  await explore.reset();
  await expect(screen.getByRole('button', 'Start')).toBeVisible();

  // The start cover names them; the masthead names the repository too.
  const cover = screen.getByRole('main');
  await expect(cover.getByText('drafts-demo', { exact: true })).toBeVisible();
  await expect(cover.getByText('kmzqvtyx', { exact: true })).toBeVisible();
  // The revision's prefix stands apart from the rest of it, as in the pane's header.
  await expect(cover.getByText('km', { exact: true })).toBeVisible();
  await expect(cover.getByText("Keep the reviewer's draft when a round reopens", { exact: false })).toBeVisible();
});
