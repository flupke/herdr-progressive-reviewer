// Past rounds: the ⋯ menu opens the list of the repository's saved rounds, by change, in the
// chat's place, and the list opens one read only, at an address of its own that a reload keeps
// and Back leaves; the page says which round it shows, that the current round waits meanwhile,
// and leads back to it. Beside the page the list stays open from one round to the next; over the
// page it closes as a round opens. A round the reviewer resets is listed at once, on the start
// cover too. The pane opens the page on the list, with no round running.
import { expect } from 'e2e';
import { test } from './session.ts';

test('the reviewer reads a past round of another change, then goes back to the current round', async ({
  explore,
  screen,
  browser,
}) => {
  // Too narrow for the list to stand beside the page: it lies over the page.
  await browser.setViewport({ width: 1280, height: 800 });
  await explore.open();
  await explore.askQuestion();
  await screen.getByRole('link', 'Go to question 1').tap();
  await expect(screen.getByRole('region', 'Question 1')).toBeVisible();

  await screen.getByRole('button', 'Round menu').tap();
  await screen.getByRole('button', /^Past rounds/).tap();
  const list = screen.getByRole('navigation', 'Past rounds');
  await expect(list).toContainText('Name the queue');
  await list.getByRole('link').first().tap();
  // At this width the list lies over the page: it closes, so that the round shows whole.
  await expect(list).toBeHidden();

  const banner = screen.getByRole('region', 'Past round');
  await expect(banner).toContainText('across reloads');
  await expect(banner).toContainText('The current round waits for you.');
  await expect(screen.getByRole('region', 'Conclusion')).toBeVisible();
  // Nothing on it acts: no Implement, no Reset, nothing to write.
  await expect(screen.getByRole('button', 'Implement')).toHaveCount(0);
  await expect(screen.getByRole('textbox')).toHaveCount(0);
  await screen.getByRole('button', 'Round menu').tap();
  await expect(screen.getByRole('button', /^Reset this round/)).toHaveCount(0);
  await screen.getByRole('button', 'Round menu').tap();

  // A step of its rail opens that step of the past round, and a reload keeps it.
  await screen.getByRole('navigation', 'Round').getByRole('link', 'Q1').tap();
  await expect(screen.getByRole('region', 'Question 1 · answered')).toBeVisible();
  await expect(browser).toHaveURL(/#question-1@session-\d+-past-1-1$/);
  await browser.reload();
  await expect(screen.getByRole('region', 'Question 1 · answered')).toBeVisible();
  await expect(banner).toBeVisible();

  // Back returns to the past round's conclusion, then the banner leads to the current round.
  await browser.evaluate(() => history.back());
  await expect(screen.getByRole('region', 'Conclusion')).toBeVisible();
  await banner.getByRole('link', 'Back to the current round').tap();
  await expect(banner).toBeHidden();
  await expect(screen.getByRole('region', 'Question 1')).toBeVisible();
  await expect(screen.getByRole('button', 'Send answer')).toBeVisible();
  expect(await explore.answers()).toEqual([]);
});

test('a round the reviewer resets is listed on the start cover and opens where it stopped', async ({
  explore,
  screen,
}) => {
  await explore.open();
  await explore.askQuestion();
  await screen.getByRole('link', 'Go to question 1').tap();
  await explore.reset();
  await expect(screen.getByRole('button', 'Start')).toBeVisible();

  await screen.getByRole('button', 'Round menu').tap();
  await screen.getByRole('button', /^Past rounds/).tap();
  const list = screen.getByRole('navigation', 'Past rounds');
  // The reset round's change comes first: it has the newest round.
  await expect(list.getByRole('region').first()).toContainText('when a round reopens');
  await list.getByRole('link').first().tap();

  const question = screen.getByRole('region', 'Question 1');
  await expect(question).toBeVisible();
  await expect(screen.getByRole('region', 'Past round')).not.toContainText('waits for you');
  // The question it left waiting shows its choices, with nothing to pick or send.
  await expect(screen.getByRole('group', 'Choices')).toBeDisabled();
  await expect(screen.getByRole('button', 'Send answer')).toHaveCount(0);
});

test('the address the pane opens shows the past rounds over the start cover, with no round running', async ({
  explore,
  screen,
  browser,
}) => {
  await explore.reset();
  await explore.openPastRounds();

  const list = screen.getByRole('navigation', 'Past rounds');
  await expect(list).toContainText('Name the queue');
  await expect(screen.getByRole('button', 'Start')).toBeVisible();
  // The address names the stage once the list shows: a reload opens no list.
  await expect(browser).toHaveURL(/#round$/);

  await list.getByRole('link').first().tap();
  await expect(screen.getByRole('region', 'Past round')).toBeVisible();
  expect(await explore.starts()).toEqual([]);
});

test('beside the page, the list stays open from one past round to the next, where the chat stands', async ({
  explore,
  screen,
  browser,
}) => {
  // Wide enough for the list to stand beside the page.
  await browser.setViewport({ width: 1920, height: 1000 });
  await explore.open();
  await explore.askQuestion();
  await screen.getByRole('link', 'Go to question 1').tap();
  await screen.getByRole('button', 'Talk to the agent').tap();
  const chat = screen.getByRole('complementary', 'Conversation with the agent');
  await expect(chat).toBeVisible();

  // The list takes the chat's place.
  await screen.getByRole('button', 'Round menu').tap();
  await screen.getByRole('button', /^Past rounds/).tap();
  const list = screen.getByRole('navigation', 'Past rounds');
  await expect(list).toBeVisible();
  await expect(chat).toBeHidden();

  await list.getByRole('link').first().tap();
  const banner = screen.getByRole('region', 'Past round');
  await expect(banner).toContainText('across reloads');
  await expect(list.getByRole('link').first()).toHaveAttribute('aria-current', 'page');
  await list.getByRole('link').last().tap();
  await expect(banner).toContainText('flush policy');
  await expect(list).toBeVisible();
  await expect(list.getByRole('link').last()).toHaveAttribute('aria-current', 'page');
  await expect(list.getByRole('link').first()).not.toHaveAttribute('aria-current', 'page');

  // The chat takes its place back.
  await screen.getByRole('button', 'Talk to the agent').tap();
  await expect(chat).toBeVisible();
  await expect(list).toBeHidden();
});
