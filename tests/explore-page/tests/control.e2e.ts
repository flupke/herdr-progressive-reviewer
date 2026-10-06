// The reviewer controls the round from the page, as from the reviewer's Explore tab: Stop
// waiting, Retry, Cancel answer, Reset. `explore.actions()` lists the actions the page sent, as
// the session took them.
import { expect } from 'e2e';
import { test } from './session.ts';

test('the reviewer answers, stops waiting for the agent, then sends the answer again with Retry', async ({
  explore,
  screen,
}) => {
  await explore.open();
  await explore.askQuestion();
  await screen.getByRole('link', 'Go to question 1').tap();
  const question = screen.getByRole('region', 'Question 1');
  await question.getByRole('radio', 'Discard the draft').check();
  await question.getByRole('button', 'Send answer').tap();
  const sent = screen.getByRole('region', 'Your answer to question 1');
  await expect(screen.getByRole('status')).toContainText('The agent is working on your answer to question 1');

  await sent.getByRole('button', 'Stop waiting').tap();
  await expect(screen.getByRole('status')).toContainText('The turn is paused');
  await expect(sent.getByRole('radio', 'Discard the draft')).toBeChecked();
  expect(await explore.actions()).toEqual(['stop']);

  await sent.getByRole('button', 'Retry').tap();
  await expect(screen.getByRole('status')).toContainText('The agent is working on your answer to question 1');
  expect(await explore.actions()).toEqual(['stop', 'retry']);
  expect(await explore.answers()).toEqual([{ question: 'keep-draft', version: 1, choice: 'discard', comment: '' }]);
});

test('the reviewer cancels the last answer, and its question waits again', async ({ explore, screen }) => {
  await explore.open();
  await explore.askQuestion();
  await screen.getByRole('link', 'Go to question 1').tap();
  await explore.answerInPane();
  await expect(screen.getByRole('region', 'Your answer to question 1').getByRole('radio', 'Keep the draft')).toBeChecked();

  // Cancel this answer asks for a confirmation first, and takes nothing back yet.
  await screen.getByText('Cancel this answer…').tap();
  expect(await explore.actions()).toEqual([]);
  await screen.getByRole('button', 'Confirm: cancel my answer').tap();
  await expect(screen.getByRole('button', 'Send answer')).toBeVisible();
  expect(await explore.actions()).toEqual(['cancel-answer']);
  await expect(screen.getByRole('region', 'Your answer to question 1')).toHaveCount(0);
});

test('the reviewer resets the round from the menu, then starts a new one', async ({ explore, screen }) => {
  await explore.open();
  await explore.askQuestion();
  await screen.getByRole('link', 'Go to question 1').tap();
  await expect(screen.getByRole('region', 'Question 1')).toBeVisible();

  await screen.getByRole('button', 'Round menu').tap();
  await screen.getByRole('button', 'Reset this round… closes it for good').tap();
  await screen.getByRole('button', 'Confirm reset').tap();
  await expect(screen.getByRole('status')).toContainText('No round is running', { ignoreCase: true });
  expect(await explore.actions()).toEqual(['reset']);

  await screen.getByRole('button', 'Start').tap();
  await expect(screen.getByRole('status')).toContainText('Preparing the round');
  expect(await explore.starts()).toEqual([{ challenger: false }]);
});
