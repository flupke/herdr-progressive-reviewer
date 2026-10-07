// The chat: the round's conversation with the agent, a review thread attached to the round. The
// reviewer asks, challenges or adds context there without answering the question; the agent's
// reply lands in the chat, and the question stays open. `explore.messages()` lists what the
// review threads saved of the reviewer's messages, `explore.agentReplies()` plays the agent's
// reply.
import { expect, type Locator } from 'e2e';
import { CHAT_REPLY, test } from './session.ts';

/** The chat, by the name of its region. */
const CHAT = 'Conversation with the agent';

/** The chat's width as its edge says it: fitted to the messages, or set by the reviewer. */
const FITTED = /^\d+ pixels, fitted to the messages$/;
const SET = /^\d+ pixels$/;

/** Checks that the chat is wider than it can be at its narrowest, as its edge says. */
async function expectWiderThanNarrowest(edge: Locator): Promise<void> {
  await expect(edge).toHaveAttribute('aria-valuenow', /\d/);
  expect(Number(await edge.getAttribute('aria-valuenow'))).toBeGreaterThan(Number(await edge.getAttribute('aria-valuemin')));
}

test('the reviewer asks from a question, sees the unread reply on the bubble, reads it, and the question stays open', async ({
  explore,
  screen,
  browser,
}) => {
  await explore.open();
  await explore.askQuestion();
  await screen.getByRole('link', 'Go to question 1').tap();

  const bubble = screen.getByRole('button', 'Talk to the agent');
  await bubble.tap();
  const chat = screen.getByRole('complementary', CHAT);
  await chat.getByRole('textbox', 'Message to the agent').fill('Why keep a draft nobody sent?');
  await chat.getByRole('button', 'Send').tap();
  await expect(chat.getByRole('status')).toContainText('The agent is answering');
  const [sent] = await explore.messages();
  expect(sent.text).toBe('Why keep a draft nobody sent?');
  // The message names its question with the number the page shows it by, for the agent.
  expect(sent.asked_under).toEqual({ stage: 'question', question: 'keep-draft', version: 1, number: 1 });
  await chat.getByRole('button', 'Close the conversation').tap();

  await explore.agentReplies();
  await expect(bubble).toHaveText('1');
  await expect(browser).toHaveTitle(/^\(1\) Q1 · your turn — /);

  await bubble.tap();
  await expect(chat.getByRole('article', 'Reply from the agent')).toContainText(CHAT_REPLY);
  await expect(browser).toHaveTitle(/^Q1 · your turn — /);
  await chat.getByRole('button', 'Close the conversation').tap();
  await expect(bubble).toHaveText('');
  // The message answered nothing: question 1 still waits for its answer.
  expect(await explore.answers()).toEqual([]);
  await expect(screen.getByRole('region', 'Question 1')).toBeVisible();
});

test('a passage selected in the question goes to the chat as a quote', async ({ explore, screen, browser }) => {
  await explore.open();
  await explore.askQuestion();
  await screen.getByRole('link', 'Go to question 1').tap();
  // The reviewer selects the question's text, then chooses the one option the selection offers.
  await browser.evaluate(() => {
    const question = document.querySelector('.question-text');
    if (!question) throw new Error('no question text');
    const range = document.createRange();
    range.selectNodeContents(question);
    document.getSelection()?.removeAllRanges();
    document.getSelection()?.addRange(range);
  });
  await screen.getByRole('button', 'Add to chat').tap();

  const chat = screen.getByRole('complementary', CHAT);
  await expect(chat).toContainText("“Should a reopened round keep the reviewer's unsent draft?”");
  await chat.getByRole('textbox', 'Message to the agent').fill('Unsent since when?');
  await chat.getByRole('button', 'Send').tap();
  await expect(chat.getByRole('article', 'Your message')).toContainText('Unsent since when?');
  const [sent] = await explore.messages();
  expect(sent.quote).toBe("Should a reopened round keep the reviewer's unsent draft?");
});

test('a message that did not reach the agent is sent again with Retry', async ({ explore, screen }) => {
  await explore.open();
  await explore.askQuestion();
  await screen.getByRole('button', 'Talk to the agent').tap();
  const chat = screen.getByRole('complementary', CHAT);
  await chat.getByRole('textbox', 'Message to the agent').fill('Who reads the draft?');
  await chat.getByRole('button', 'Send').tap();
  await expect(chat.getByRole('status')).toContainText('The agent is answering');
  await explore.messagesNotDelivered();
  await expect(chat.getByRole('alert')).toContainText('Your message did not reach the agent');

  await chat.getByRole('button', 'Retry').tap();
  await expect(chat.getByRole('status')).toContainText('The agent is answering');
  expect(await explore.actions()).toEqual(['retry-messages']);
});

test('a message being typed in the chat comes back after a reload', async ({ explore, screen }) => {
  await explore.open();
  await explore.askQuestion();
  await screen.getByRole('button', 'Talk to the agent').tap();
  const chat = screen.getByRole('complementary', CHAT);
  await chat.getByRole('textbox', 'Message to the agent').fill('Half a thought');
  await explore.open();

  await screen.getByRole('button', 'Talk to the agent').tap();
  await expect(chat.getByRole('textbox', 'Message to the agent')).toHaveValue('Half a thought');
});

test('a reply to the conclusion keeps its text and its focus while the implementation request goes out', async ({
  explore,
  screen,
}) => {
  await explore.open();
  await explore.conclude();
  await screen.getByRole('button', 'Implement', { exact: true }).tap();
  await expect(screen.getByRole('status')).toContainText('Sending the implementation request');
  await screen.getByRole('button', 'Not ready? Reply to the agent instead').tap();
  const reply = screen.getByRole('textbox', 'Message to the agent');
  await reply.fill('Keep the old name for one release.');

  await explore.deliverImplementation();
  await expect(screen.getByRole('status')).toContainText('The agent received the implementation request');
  await expect(reply).toHaveValue('Keep the old name for one release.');
  // A page drawn again from scratch would have taken the focus from the message.
  await expect(reply).toBeFocused();

  await screen.getByRole('complementary', CHAT).getByRole('button', 'Send').tap();
  await expect(screen.getByRole('complementary', CHAT).getByRole('status')).toContainText('The agent is answering');
  const [sent] = await explore.messages();
  expect(sent.text).toBe('Keep the old name for one release.');
  expect(sent.asked_under).toEqual({ stage: 'conclusion', conclusion: 'conclusion' });
});

test('a wide reply widens the chat beside the page; the width the reviewer sets stays across a reload until the fitted one comes back', async ({
  explore,
  screen,
  browser,
}) => {
  // Wide enough for the chat to stand beside the page and widen.
  await browser.setViewport({ width: 1920, height: 1000 });
  await explore.open();
  await explore.askQuestion();
  await screen.getByRole('link', 'Go to question 1').tap();
  await screen.getByRole('button', 'Talk to the agent').tap();
  const chat = screen.getByRole('complementary', CHAT);
  await chat.getByRole('textbox', 'Message to the agent').fill('Who calls flush?');
  await chat.getByRole('button', 'Send').tap();
  await explore.agentRepliesWide();
  await expect(chat.getByRole('table')).toBeVisible();

  const edge = chat.getByRole('separator', 'Width of the chat');
  await edge.focus();
  await expect(edge).toHaveAttribute('aria-valuetext', FITTED);
  await expectWiderThanNarrowest(edge);
  await edge.press('Home');
  await expect(edge).toHaveAttribute('aria-valuetext', SET);

  await browser.reload();
  await screen.getByRole('button', 'Talk to the agent').tap();
  await edge.focus();
  await expect(edge).toHaveAttribute('aria-valuetext', SET);
  await edge.press('Enter');
  await expect(edge).toHaveAttribute('aria-valuetext', FITTED);
  await expectWiderThanNarrowest(edge);
});
