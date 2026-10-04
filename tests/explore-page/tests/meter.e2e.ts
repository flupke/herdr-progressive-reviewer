import { expect } from 'e2e';
import { test } from './session.ts';

// The meter on the masthead's hairline says how much of the change the review marks cover. The
// fixture's change has three changed lines, one of which Jev marked when the round started; an
// answer to the first question marks the other two. The meter's name carries its numbers for a
// screen reader, and its window shows them.

// What the reviewer calls the meter: the line under the masthead, which names itself. The
// first question opens on the design screen, which the agent reads past before it answers: a
// run that records the steps takes longer than the usual 30 seconds.
const METER = {
  agentContext:
    'The meter is the line under the masthead, a button named "Lines reviewed: …"; a click on it pins its details open.',
  timeout: 90_000,
};

test('an answer advances the reviewed share of the change', METER, async ({ explore, screen, agent }) => {
  await explore.open();
  await explore.askQuestion();
  const meter = screen.getByRole('button', /^Lines reviewed/);
  await expect(meter).toHaveAccessibleName('Lines reviewed: 33%, 1 of 3 changed lines');
  await agent.act('open the details of the "Lines reviewed" meter');
  const details = screen.getByRole('group', 'Review marks of the change');
  await expect(details.getByText('33% reviewed')).toBeVisible();
  await expect(details.getByText('This question marks when you answer')).toBeVisible();

  await agent.act('answer question 1 with "Keep the draft" and send it');
  await expect(meter).toHaveAccessibleName('Lines reviewed: 100%, 3 of 3 changed lines');
  await agent.act('open the details of the "Lines reviewed" meter again');
  await expect(details.getByText('100% reviewed')).toBeVisible();
  await expect(details.getByText('Your answers · Q1')).toBeVisible();
});

test("the meter's window opens and closes with the keyboard", async ({ explore, screen }) => {
  await explore.open();
  await explore.askQuestion();
  const meter = screen.getByRole('button', /^Lines reviewed/);
  const details = screen.getByRole('group', 'Review marks of the change');
  await expect(details).toBeHidden();

  // The keys themselves are what this test checks, so it presses them rather than asking the
  // agent, which could open the window with the pointer.
  await meter.focus();
  await meter.press('Enter');
  await expect(meter).toBeExpanded();
  await expect(details.getByText('src/drafts.rs')).toBeVisible();

  await meter.press('Escape');
  await expect(details).toBeHidden();
  await expect(meter).toBeFocused();
});

test('a mark by hand during a round reaches the meter at once', async ({ explore, screen }) => {
  await explore.open();
  await explore.askQuestion();
  const meter = screen.getByRole('button', /^Lines reviewed/);
  await expect(meter).toHaveAccessibleName('Lines reviewed: 33%, 1 of 3 changed lines');

  await explore.markByHand();
  await expect(meter).toHaveAccessibleName('Lines reviewed: 100%, 3 of 3 changed lines');
});

test('the start cover says how large the change is', async ({ explore, screen }) => {
  await explore.open();
  await explore.reset();
  await expect(screen.getByText(/\+2 −1 in 1 file$/)).toBeVisible();
});
