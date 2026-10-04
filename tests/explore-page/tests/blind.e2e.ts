// The blind first pick: a question that is hard to reverse hides the agent's recommendation until
// the reviewer's first Send, which records the pick and sends no answer; Confirm answer then
// sends it. A two-way question shows the recommendation at once and sends with one Send.
import { expect } from 'e2e';
import type { Screen } from 'e2e';
import { test } from './session.ts';

const RECOMMENDATION = 'Renaming keeps every file, and the new names stay readable.';

/** The agent's alternatives, by ID, in the order it posted them. */
const POSTED = ['rename', 'overwrite', 'stop'];

/** A question whose door is `door`, which recommends renaming. */
function question(door: string) {
  const consequence = (summary: string) => ({ summary, evidence: [], unknowns: ['No caller was checked.'] });
  return {
    id: 'same-name-files',
    version: 1,
    topic: 'export',
    text: 'What should the export do with two files of the same name?',
    rationale: null,
    visual: null,
    alternatives: [
      { id: 'rename', text: 'Rename the clashing files', outcome: 'accepted', recommendation: RECOMMENDATION },
      { id: 'overwrite', text: 'Overwrite the older file', outcome: 'needs_follow_up', recommendation: null },
      { id: 'stop', text: 'Stop the export with an error', outcome: 'needs_follow_up', recommendation: null },
    ],
    evidence: [],
    assessments: {
      door,
      reversibility: consequence('Files an export overwrote are gone.'),
      blast_radius: consequence('Every export with clashing names.'),
    },
  };
}

/** What the first Send of a blind question achieves, which a goal names as its end. */
const BLIND = {
  agentContext:
    "On a blind question, the first press of Send answer sends no answer: it shows the agent's recommendation on the recommended choice, and the button becomes Confirm answer. Pressing Send answer once is done as soon as Confirm answer shows. Confirm answer then sends the answer, and the page says that the agent is working.",
};

/** The choices the page offers, by ID, in its order. */
async function choiceOrder(screen: Screen): Promise<(string | null)[]> {
  const radios = await screen.getByRole('radio').all();
  return Promise.all(radios.map((radio) => radio.getAttribute('value')));
}

test('on a blind question, the first Send shows the recommendation, and the reviewer changes the pick, then confirms', BLIND, async ({
  explore,
  screen,
  agent,
  browser,
}) => {
  await explore.open();
  await explore.askQuestion(question('one_way'));
  await screen.getByRole('link', 'Go to question 1').tap();
  const question1 = screen.getByRole('region', 'Question 1');
  await expect(question1).toContainText('Blind pick');

  // The agent's alternatives in a mixed order, None of the above last, and none selected.
  const shown = await choiceOrder(screen);
  expect(shown).not.toEqual([...POSTED, 'none-of-the-above']);
  expect([...shown].sort()).toEqual([...POSTED, 'none-of-the-above'].sort());
  await expect(screen.getByRole('radio', { checked: true })).toHaveCount(0);
  await expect(screen.getByText(RECOMMENDATION, { exact: false })).toHaveCount(0);

  await agent.act(
    'choose "Overwrite the older file" on question 1 and press Send answer once, which shows the agent\'s recommendation and Confirm answer',
  );
  // The first Send records the pick and sends no answer: the recommendation shows, with the line
  // that says the agent picked another choice, and the button now confirms.
  await expect(screen.getByText(RECOMMENDATION, { exact: false })).toBeVisible();
  await expect(screen.getByText('The agent recommends another choice.')).toBeVisible();
  await expect(screen.getByRole('radio', 'Overwrite the older file')).toBeChecked();
  await expect(screen.getByRole('button', 'Confirm answer')).toBeVisible();
  expect(await explore.answers()).toEqual([]);
  expect(await choiceOrder(screen)).toEqual(shown);
  // The reveal is in view without a scroll by the reviewer, a phone's included. A position has
  // no locator: it is read in the page.
  const reveal = await browser.evaluate(() => {
    const box = document.querySelector('.reveal')!.getBoundingClientRect();
    return { top: box.top, bottom: box.bottom, height: window.innerHeight };
  });
  expect(reveal.top).toBeGreaterThanOrEqual(0);
  expect(reveal.bottom).toBeLessThanOrEqual(reveal.height);
  // The reason describes the recommended choice.
  await expect(screen.getByRole('radio', 'Rename the clashing files')).toHaveAttribute('aria-describedby');

  await agent.act(
    'on question 1, select the choice the agent recommends instead of your pick, and confirm the answer; the page then says that the agent is working',
  );
  await expect(screen.getByRole('status')).toContainText('The agent is working');
  expect(await explore.answers()).toEqual([
    { question: 'same-name-files', version: 1, choice: 'rename', comment: '', first_pick: 'overwrite' },
  ]);
});

test('a first pick that is the recommended choice says that the reviewer and the agent agree', async ({
  explore,
  screen,
}) => {
  await explore.open();
  await explore.askQuestion(question('mixed'));
  await screen.getByRole('link', 'Go to question 1').tap();
  // Exact actions: the outcome of this exact pick is the point of the test.
  await screen.getByRole('radio', 'Rename the clashing files').check();
  await screen.getByRole('button', 'Send answer').tap();
  await expect(screen.getByText('You and the agent picked the same choice.')).toBeVisible();
  await expect(screen.getByText(RECOMMENDATION, { exact: false })).toBeVisible();
  expect(await explore.answers()).toEqual([]);
});

test('a two-way question shows the recommendation at once and sends with one Send', async ({ explore, screen, agent }) => {
  await explore.open();
  await explore.askQuestion(question('two_way'));
  await screen.getByRole('link', 'Go to question 1').tap();
  await expect(screen.getByRole('region', 'Question 1')).toBeVisible();

  await expect(screen.getByText(RECOMMENDATION, { exact: false })).toBeVisible();
  expect(await choiceOrder(screen)).toEqual([...POSTED, 'none-of-the-above']);
  await expect(screen.getByRole('region', 'Question 1')).not.toContainText('Blind pick');

  // One Send answers.
  await agent.act(
    'on question 1, select the choice the agent recommends and send the answer; the page then says that the agent is working',
  );
  await expect(screen.getByRole('status')).toContainText('The agent is working');
  expect(await explore.answers()).toEqual([{ question: 'same-name-files', version: 1, choice: 'rename', comment: '' }]);
});

test('a pick on a question answered in the pane meanwhile is refused', async ({ explore, screen }) => {
  await explore.open();
  await explore.askQuestion(question('one_way'));
  await screen.getByRole('link', 'Go to question 1').tap();
  // The page shows the question, and is held there while the pane answers it.
  await expect(screen.getByRole('region', 'Question 1')).toBeVisible();
  await explore.holdPage();
  await explore.answerInPane();

  // An exact action: the refusal of this pick is the point of the test, which a goal to pick
  // would count as a failure.
  await screen.getByRole('radio', 'Overwrite the older file').check();
  await screen.getByRole('button', 'Send answer').tap();
  await expect(screen.getByRole('alert')).toContainText('so your pick was not kept');
  await expect(screen.getByRole('status')).toContainText('The agent is working');
});

test('a question asked again after Cancel answer shows the recommendation at once and keeps no first pick', async ({
  explore,
  screen,
  agent,
}) => {
  await explore.open();
  await explore.askQuestion(question('one_way'));
  await screen.getByRole('link', 'Go to question 1').tap();
  // Exact actions: the setup is a pick kept on the page, then an answer in the pane, which the
  // reviewer cancels there.
  await screen.getByRole('radio', 'Overwrite the older file').check();
  await screen.getByRole('button', 'Send answer').tap();
  await expect(screen.getByText(RECOMMENDATION, { exact: false })).toBeVisible();
  await explore.answerInPane();
  await explore.cancelAnswerInPane();

  await explore.open();
  await expect(screen.getByRole('region', 'Question 1')).toBeVisible();
  await expect(screen.getByText(RECOMMENDATION, { exact: false })).toBeVisible();
  await expect(screen.getByRole('button', 'Confirm answer')).toHaveCount(0);
  await expect(screen.getByRole('radio', { checked: true })).toHaveCount(0);
  expect(await choiceOrder(screen)).toEqual([...POSTED, 'none-of-the-above']);

  await agent.act(
    'on question 1, select the choice the agent recommends and press Send; the page then says that the agent is working',
  );
  await expect(screen.getByRole('status')).toContainText('The agent is working');
  expect(await explore.answers()).toEqual([{ question: 'same-name-files', version: 1, choice: 'rename', comment: '' }]);
});

const COMMENT = 'Renaming hides which file the user meant to keep.';

test('a comment written before the first Send stays in the box until the answer is confirmed', BLIND, async ({
  explore,
  screen,
  agent,
}) => {
  await explore.open();
  await explore.askQuestion(question('one_way'));
  await screen.getByRole('link', 'Go to question 1').tap();
  const comment = screen.getByRole('textbox', 'Comment · optional');
  await expect(comment).toBeVisible();

  await agent.act(
    'on question 1, write {comment} as the comment, choose "Overwrite the older file" and press Send answer once, which shows Confirm answer; do not confirm yet',
    { params: { comment: COMMENT } },
  );
  await expect(screen.getByText(RECOMMENDATION, { exact: false })).toBeVisible();
  await expect(comment).toHaveValue(COMMENT);
  // The comment is not an answer until the reviewer sends it.
  expect(await explore.answers()).toEqual([]);

  await agent.act('confirm the answer to question 1 as it is; the page then says that the agent is working');
  await expect(screen.getByRole('status')).toContainText('The agent is working');
  expect(await explore.answers()).toEqual([
    { question: 'same-name-files', version: 1, choice: 'overwrite', comment: COMMENT, first_pick: 'overwrite' },
  ]);
});
