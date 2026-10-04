// Between an answer and the next question: the page shows the agent at work on the answer, with
// the time since it was sent, the answered question read again, and in the panel the answer
// that was sent with Stop waiting. A turn that did not go through shows its card in the same
// place, and the panel offers its one action, Retry.
import { expect } from 'e2e';
import { test } from './session.ts';

const COMMENT = 'Keep it for a week instead.';

// What each goal achieves on the page, which the agent names as its end.
const WAITING = {
  agentContext:
    'Answering question 1 on this page is done once the page says that the agent is working on the answer. Stop waiting is done once the page says that the turn is paused. Retry is done once the page says that the agent is working again.',
};

test('the reviewer answers, sees the sent answer while the agent works, stops waiting, then retries', WAITING, async ({
  explore,
  screen,
  agent,
}) => {
  await explore.open();
  await explore.askQuestion();
  await screen.getByRole('link', 'Go to question 1').tap();

  await agent.act('answer question 1 with "Discard the draft", write {comment} as the comment, and send it', {
    params: { comment: COMMENT },
  });
  const working = screen.getByRole('status');
  await expect(working).toContainText('The agent is working on your answer to question 1');
  // The panel holds what was sent; the question stays on the desk, answered.
  const sent = screen.getByRole('region', 'Your answer to question 1');
  await expect(sent).toContainText('Discard the draft');
  await expect(sent).toContainText(COMMENT);
  await expect(screen.getByRole('region', 'Question 1 · answered')).toContainText(
    "Should a reopened round keep the reviewer's unsent draft?",
  );
  // The time since the answer was sent counts on in the page.
  await expect(working).toContainText(/Sent 0:0\d ago/);
  await expect(working).toContainText(/Sent 0:0[3-9] ago/, { timeout: 10_000 });

  await agent.act('stop waiting for the agent');
  await expect(screen.getByRole('status')).toContainText('The turn is paused');
  await expect(sent).toContainText('Discard the draft');
  expect(await explore.actions()).toEqual(['stop']);

  await agent.act('send the answer to the agent again with Retry');
  await expect(screen.getByRole('status')).toContainText('The agent is working on your answer to question 1');
  expect(await explore.actions()).toEqual(['stop', 'retry']);
  expect(await explore.answers()).toEqual([
    { question: 'keep-draft', version: 1, choice: 'discard', comment: COMMENT },
  ]);
});

test('an answer whose prompt did not reach the agent shows the failure, the answer, and Retry in the panel', async ({
  explore,
  screen,
}) => {
  await explore.open();
  await explore.askQuestion();
  await explore.answerInPane();
  await explore.failDelivery();

  await expect(screen.getByRole('alert')).toContainText('The selected agent is no longer available');
  const sent = screen.getByRole('region', 'Your answer to question 1');
  await expect(sent).toContainText('Keep the draft');
  await expect(sent.getByRole('button', 'Retry')).toBeVisible();
  await expect(screen.getByRole('region', 'Question 1 · answered')).toBeVisible();
});

test('the moving bar of a working card stands still when the reviewer asks for less motion', async ({
  explore,
  screen,
  browser,
}) => {
  await explore.open();
  await expect(screen.getByRole('status')).toContainText('The agent is working');
  // e2e's browser cannot emulate `prefers-reduced-motion`, and no locator reads an animation: the
  // page's own styles say what the bar does with and without the reviewer's request.
  const bar = await browser.evaluate(() => {
    const running = getComputedStyle(document.querySelector('.status-bar')!, '::before').animationName;
    const still = [...document.styleSheets]
      .flatMap((sheet) => [...sheet.cssRules])
      .filter((rule): rule is CSSMediaRule => rule instanceof CSSMediaRule)
      .filter((rule) => rule.conditionText.includes('prefers-reduced-motion: reduce'))
      .flatMap((rule) => [...rule.cssRules])
      .filter((rule): rule is CSSStyleRule => rule instanceof CSSStyleRule)
      .some((rule) => rule.selectorText === '.status-bar::before' && rule.style.animationName === 'none');
    return { running, still };
  });
  expect(bar).toEqual({ running: 'status-run', still: true });
});
