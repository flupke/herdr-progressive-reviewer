// Past rounds: the ⋯ menu lists the repository's saved rounds by change, and opens one read
// only, at an address of its own that a reload keeps and Back leaves; the page says which round
// it shows, that the current round waits meanwhile, and leads back to it. A round the reviewer
// resets is listed at once, on the start cover too.
import { expect } from 'e2e';
import { test } from './session.ts';

test('the reviewer reads a past round of another change, then goes back to the current round', async ({
  explore,
  screen,
  browser,
}) => {
  await explore.open();
  await explore.askQuestion();
  await screen.getByRole('link', 'Go to question 1').tap();
  await expect(screen.getByRole('region', 'Question 1')).toBeVisible();

  await screen.getByRole('button', 'Round menu').tap();
  await screen.getByRole('button', /^Past rounds/).tap();
  const list = screen.getByRole('navigation', 'Past rounds');
  await expect(list).toContainText('Name the queue');
  await list.getByRole('link').first().tap();

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
