// The blind first pick: a question that is hard to reverse hides the agent's recommendation until
// the reviewer has picked an answer; a two-way question shows it at once.
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

/** The choices the page offers, by ID, in its order. */
async function choiceOrder(screen: Screen): Promise<(string | null)[]> {
  const radios = await screen.getByRole('radio').all();
  return Promise.all(radios.map((radio) => radio.getAttribute('value')));
}

test('a question that is hard to reverse hides the recommendation until the reviewer picks', async ({
  explore,
  screen,
  agent,
}) => {
  await explore.open();
  await explore.askQuestion(question('one_way'));
  await expect(screen.getByRole('region', 'Question 1')).toBeVisible();

  // The agent's alternatives in a mixed order, None of the above last, and none selected.
  const shown = await choiceOrder(screen);
  expect(shown).not.toEqual([...POSTED, 'none-of-the-above']);
  expect([...shown].sort()).toEqual([...POSTED, 'none-of-the-above'].sort());
  await expect(screen.getByRole('radio', { checked: true })).toHaveCount(0);
  await expect(screen.getByText(RECOMMENDATION, { exact: false })).toHaveCount(0);

  await agent.act('choose "Overwrite the older file" on question 1 and press Pick, without sending an answer yet');
  await expect(screen.getByText(RECOMMENDATION, { exact: false })).toBeVisible();
  await expect(screen.getByRole('radio', 'Overwrite the older file')).toBeChecked();
  expect(await choiceOrder(screen)).toEqual(shown);
  // The reason describes the recommended choice.
  await expect(screen.getByRole('radio', 'Rename the clashing files')).toHaveAttribute('aria-describedby');

  await agent.act(
    'on question 1, select the choice the agent recommends instead of your pick, and press Send; the page then says that the agent is working',
  );
  await expect(screen.getByRole('status')).toContainText('The agent is working');
  expect(await explore.answers()).toEqual([
    { question: 'same-name-files', version: 1, choice: 'rename', comment: '', first_pick: 'overwrite' },
  ]);
});

test('a two-way question shows the recommendation at once', async ({ explore, screen }) => {
  await explore.open();
  await explore.askQuestion(question('two_way'));
  await expect(screen.getByRole('region', 'Question 1')).toBeVisible();

  await expect(screen.getByText(RECOMMENDATION, { exact: false })).toBeVisible();
  expect(await choiceOrder(screen)).toEqual([...POSTED, 'none-of-the-above']);
  await expect(screen.getByRole('button', 'Send')).toBeVisible();
});

test('a pick on a question answered in the pane meanwhile is refused', async ({ explore, screen }) => {
  await explore.open();
  await explore.askQuestion(question('one_way'));
  // The page shows the question, and does not follow the answer given in the pane.
  await expect(screen.getByRole('region', 'Question 1')).toBeVisible();
  await explore.answerInPane();

  // An exact action: the refusal of this pick is the point of the test, which a goal to pick
  // would count as a failure.
  await screen.getByRole('radio', 'Overwrite the older file').check();
  await screen.getByRole('button', 'Pick').tap();
  await expect(screen.getByRole('alert')).toContainText('Your pick was not kept');
  await expect(screen.getByRole('status')).toContainText('The agent is working');
});

test('a question asked again after Cancel answer shows the recommendation at once and keeps no first pick', async ({
  explore,
  screen,
  agent,
}) => {
  await explore.open();
  await explore.askQuestion(question('one_way'));
  // Exact actions: the setup is a pick kept on the page, then an answer in the pane, which the
  // reviewer cancels there.
  await screen.getByRole('radio', 'Overwrite the older file').check();
  await screen.getByRole('button', 'Pick').tap();
  await expect(screen.getByText(RECOMMENDATION, { exact: false })).toBeVisible();
  await explore.answerInPane();
  await explore.cancelAnswerInPane();

  await explore.open();
  await expect(screen.getByRole('region', 'Question 1')).toBeVisible();
  await expect(screen.getByText(RECOMMENDATION, { exact: false })).toBeVisible();
  await expect(screen.getByRole('button', 'Pick')).toHaveCount(0);
  await expect(screen.getByRole('radio', { checked: true })).toHaveCount(0);
  expect(await choiceOrder(screen)).toEqual([...POSTED, 'none-of-the-above']);

  await agent.act(
    'on question 1, select the choice the agent recommends and press Send; the page then says that the agent is working',
  );
  await expect(screen.getByRole('status')).toContainText('The agent is working');
  expect(await explore.answers()).toEqual([{ question: 'same-name-files', version: 1, choice: 'rename', comment: '' }]);
});
