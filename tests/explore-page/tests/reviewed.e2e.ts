import { expect } from 'e2e';
import { test } from './session.ts';

// With every changed line marked as reviewed, a round has nothing to ask: the start screen keeps
// Start and Start with Challenger, inactive, and says why.

const NOTHING_TO_REVIEW = 'Nothing is left to review';

// What a start on the page achieves: the review tool starts the round afterwards, out of the
// page's hands.
const STARTING = {
  agentContext:
    'Starting an Explore round on this page is done once the page says that it is preparing the round.',
};

test('a fully reviewed review offers no start, and says why', async ({ explore, screen }) => {
  await explore.open();
  await explore.reviewEverything();
  // The page follows the round while the agent works, so it shows the start screen at once.
  await explore.reset();
  await expect(screen.getByRole('status')).toContainText('No Explore round is running');

  await expect(screen.getByRole('button', 'Start')).toBeDisabled();
  await expect(screen.getByRole('button', 'Start with Challenger')).toBeDisabled();
  await expect(screen.getByText(NOTHING_TO_REVIEW, { exact: false })).toBeVisible();
});

test('once a line is unreviewed, the reviewer starts a round', STARTING, async ({ explore, screen, agent }) => {
  await explore.open();
  await explore.reviewEverything();
  await explore.reset();
  await expect(screen.getByRole('button', 'Start')).toBeDisabled();

  await explore.unreviewLine();
  await agent.act('load the page again at "/"');
  await expect(screen.getByRole('button', 'Start')).toBeEnabled();
  await expect(screen.getByText(NOTHING_TO_REVIEW, { exact: false })).toBeHidden();

  await agent.act('start an Explore round without the Challenger');
  await expect(screen.getByRole('status')).toContainText('Preparing the round');
  expect(await explore.starts()).toEqual([{ challenger: false }]);
});
