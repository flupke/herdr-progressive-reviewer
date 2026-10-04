// Earlier questions: each done question on the round rail opens the question as the reviewer
// answered it, with what the agent recorded, read only, and the way back to the round. On a
// phone, a swipe turns between the design, the earlier questions and the current question; a
// table that scrolls sideways keeps the gesture for its own scroll.
import { expect, type Locator, type Screen } from 'e2e';
import { test, type Session } from './session.ts';

/** The round waits for the answer to question 2: the reviewer answered question 1 in the pane. */
async function atQuestion2(explore: Session, screen: Screen) {
  await explore.open();
  await explore.askQuestion();
  await screen.getByRole('link', 'Go to question 1').tap();
  await explore.answerInPane();
  await explore.askQuestion();
  await expect(screen.getByRole('region', 'Question 2')).toBeVisible();
}

/**
 * A sideways drag of 240 pixels across `start`, between 40 and 280 pixels from its left edge,
 * which a phone shows: `right` turns to the screen before, `left` to the one after.
 * @param direction where the finger goes
 */
async function swipe(screen: Screen, start: Locator, direction: 'left' | 'right') {
  await start.scrollIntoView();
  // The page may still settle (a diagram drawn, a view pushed, the screen a swipe turned to
  // sliding into place): wait until the start is in view.
  await expect
    .poll(async () => {
      const box = await start.boundingBox();
      return box !== null && box.y >= 0 && box.y + 20 < 844 && box.x >= 0;
    })
    .toBe(true);
  const box = await start.boundingBox();
  if (!box) throw new Error('nothing to swipe on');
  const y = Math.round(box.y + Math.min(box.height / 2, 20));
  const [from, to] = direction === 'right' ? [box.x + 40, box.x + 280] : [box.x + 280, box.x + 40];
  await screen.swipe({ from: { x: Math.round(from), y }, to: { x: Math.round(to), y } });
}

test('the reviewer reads question 1 again from the rail, then goes back to question 2', async ({
  explore,
  screen,
  agent,
  browser,
}) => {
  await browser.setViewport({ width: 1440, height: 900 });
  await atQuestion2(explore, screen);

  await agent.act('open question 1 from the round rail');
  const earlier = screen.getByRole('region', 'Question 1 · answered');
  await expect(earlier.getByRole('region', 'Your answer to question 1').getByRole('radio', 'Keep the draft')).toBeChecked();
  await expect(earlier.getByRole('region', 'The agent recorded')).toContainText('the draft stays with the round');
  // Nothing on it can change the round: its choices show the kept one, read only; no comment to
  // write, nothing to send.
  await expect(earlier.getByRole('group', 'Choices')).toBeDisabled();
  await expect(earlier.getByRole('textbox')).toHaveCount(0);
  await expect(earlier.getByRole('button', 'Send answer')).toHaveCount(0);

  await agent.act('go back to question 2');
  await expect(screen.getByRole('region', 'Question 2')).toBeVisible();
  await expect(earlier).toBeHidden();
  expect(await explore.answers()).toEqual([]);
});

test('on a phone, a swipe turns between the questions and the design, and a table scrolls without turning', async ({
  explore,
  screen,
  browser,
}) => {
  await browser.setViewport({ width: 390, height: 844 });
  await atQuestion2(explore, screen);

  // Each swipe is an exact drag rather than a goal: where it starts and how far it goes is the
  // fact under test (past the threshold, inside or outside the table).
  // From question 2, a swipe to the right turns back to question 1, then to the design.
  await swipe(screen, screen.getByRole('region', 'Question 2').getByRole('heading').first(), 'right');
  const earlier = screen.getByRole('region', 'Question 1 · answered');
  await expect(earlier).toBeVisible();
  await swipe(screen, earlier.getByRole('heading').first(), 'right');
  const design = screen.getByRole('region', 'Design of the change');
  await expect(design).toBeVisible();

  // The design's table is wider than a phone and scrolls sideways: a drag that starts in it is
  // the table's, and turns nothing.
  await swipe(screen, design.getByRole('table').first(), 'left');
  await expect(design).toBeVisible();
  await expect(earlier).toBeHidden();

  // A swipe to the left turns forward again.
  await swipe(screen, design.getByRole('heading').first(), 'left');
  await expect(earlier).toBeVisible();
});
