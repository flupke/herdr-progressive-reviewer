import { expect } from 'e2e';
import { CHAT_REPLY, test } from './session.ts';

// The chat: the round's conversation with the agent, a review thread attached to the round. The
// reviewer asks, challenges or adds context there without answering the question; the agent's
// reply lands in the chat, and the question stays open. `explore.messages()` lists what the
// review threads saved of the reviewer's messages, `explore.agentReplies()` plays the agent's
// reply, and `explore.answers()` shows that no message became an answer.

/** The chat, by the name of its region. */
const CHAT = 'Conversation with the agent';

// What sending a message achieves: it shows in the chat, waiting for the agent.
const ASKING = {
  agentContext: 'A message in the chat is sent once the chat shows it, with the agent answering it.',
};

test('the reviewer asks the agent from a question and reads its reply, and the question stays open', ASKING, async ({
  explore,
  screen,
  agent,
}) => {
  await explore.open();
  await explore.askQuestion();
  await screen.getByRole('link', 'Go to question 1').tap();

  await agent.act('ask the agent {message} in the chat', { params: { message: 'Why keep a draft nobody sent?' } });
  const chat = screen.getByRole('complementary', CHAT);
  await expect(chat.getByRole('status')).toContainText('The agent is answering');
  const [sent] = await explore.messages();
  expect(sent.text).toBe('Why keep a draft nobody sent?');
  // The message names its question with the number the page shows it by, for the agent.
  expect(sent.asked_under).toEqual({ stage: 'question', question: 'keep-draft', version: 1, number: 1 });

  await explore.agentReplies();
  await expect(chat.getByRole('article', 'Reply from the agent')).toContainText(CHAT_REPLY);
  await expect(chat.getByRole('status')).toHaveCount(0);
  // The message answered nothing: question 1 still waits for its answer.
  expect(await explore.answers()).toEqual([]);
  await expect(screen.getByRole('region', 'Question 1')).toBeVisible();
});

test('the reviewer replies to the conclusion through the chat', ASKING, async ({ explore, screen, agent }) => {
  await explore.open();
  await explore.conclude();
  await expect(screen.getByRole('region', 'Conclusion')).toBeVisible();

  await agent.act('reply {reply} to the conclusion', { params: { reply: 'Why not keep the draft in memory?' } });
  const chat = screen.getByRole('complementary', CHAT);
  await expect(chat.getByRole('status')).toContainText('The agent is answering');
  const [sent] = await explore.messages();
  expect(sent.text).toBe('Why not keep the draft in memory?');
  expect(sent.asked_under).toEqual({ stage: 'conclusion', conclusion: 'conclusion' });

  await explore.agentReplies();
  await expect(chat.getByRole('article', 'Reply from the agent')).toContainText(CHAT_REPLY);
  // The reply took no turn of the round: the conclusion stays, and nothing else was sent.
  expect(await explore.actions()).toEqual([]);
  await expect(screen.getByRole('region', 'Conclusion')).toBeVisible();
});

test("the round menu opens the agent's conversation", async ({ explore, screen, agent }) => {
  await explore.open();
  await explore.askQuestion();
  await expect(screen.getByRole('complementary', CHAT)).toBeHidden();

  await agent.act("open the agent's conversation from the round menu");
  await expect(screen.getByRole('complementary', CHAT)).toBeVisible();
});

test('a passage selected in the question goes to the chat as a quote', async ({ explore, screen, browser }) => {
  await explore.open();
  await explore.askQuestion();
  await screen.getByRole('link', 'Go to question 1').tap();
  // Exact actions: the reviewer selects the question's text, then chooses the one option the
  // selection offers.
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

test("a reply the reviewer has not seen shows on the bubble and in the tab's title", async ({
  explore,
  screen,
  browser,
}) => {
  await explore.open();
  await explore.askQuestion();
  // Exact actions: the reviewer sends a message, then closes the chat.
  await screen.getByRole('button', 'Talk to the agent').tap();
  const chat = screen.getByRole('complementary', CHAT);
  await chat.getByRole('textbox', 'Message to the agent').fill('Who reads the draft?');
  await chat.getByRole('button', 'Send').tap();
  await expect(chat.getByRole('status')).toContainText('The agent is answering');
  await chat.getByRole('button', 'Close the conversation').tap();

  await explore.agentReplies();
  const bubble = screen.getByRole('button', 'Talk to the agent');
  await expect(bubble).toHaveText('1');
  await expect(browser).toHaveTitle(/^\(1\) Q1 · your turn — /);

  await bubble.tap();
  await expect(chat.getByRole('article', 'Reply from the agent')).toContainText(CHAT_REPLY);
  await expect(browser).toHaveTitle(/^Q1 · your turn — /);
  await chat.getByRole('button', 'Close the conversation').tap();
  await expect(bubble).toHaveText('');
});

test('a message that did not reach the agent offers Retry', async ({ explore, screen }) => {
  await explore.open();
  await explore.askQuestion();
  // Exact actions: the reviewer sends a message, whose wakeup then fails.
  await screen.getByRole('button', 'Talk to the agent').tap();
  const chat = screen.getByRole('complementary', CHAT);
  await chat.getByRole('textbox', 'Message to the agent').fill('Who reads the draft?');
  await chat.getByRole('button', 'Send').tap();
  await explore.messagesNotDelivered();
  await expect(chat.getByRole('alert')).toContainText('Your message did not reach the agent');

  await chat.getByRole('button', 'Retry').tap();
  await expect(chat.getByRole('status')).toContainText('The agent is answering');
  expect(await explore.actions()).toEqual(['retry-messages']);
});

test('a message being typed in the chat comes back after a reload', async ({ explore, screen }) => {
  await explore.open();
  await explore.askQuestion();
  // Exact actions: the text must be this one, then the page loads again.
  await screen.getByRole('button', 'Talk to the agent').tap();
  const chat = screen.getByRole('complementary', CHAT);
  await chat.getByRole('textbox', 'Message to the agent').fill('Half a thought');
  await explore.open();

  await screen.getByRole('button', 'Talk to the agent').tap();
  await expect(chat.getByRole('textbox', 'Message to the agent')).toHaveValue('Half a thought');
});

test('on a phone, a sideways drag inside the open chat turns no page', async ({ explore, screen, browser }) => {
  await browser.setViewport({ width: 390, height: 844 });
  await explore.open();
  await explore.askQuestion();
  await screen.getByRole('link', 'Go to question 1').tap();
  await explore.answerInPane();
  await explore.askQuestion();
  const question = screen.getByRole('region', 'Question 2');
  await expect(question).toBeVisible();
  await screen.getByRole('button', 'Talk to the agent').tap();
  const chat = screen.getByRole('complementary', CHAT);
  await expect(chat).toBeVisible();

  // An exact drag rather than a goal: where it starts and how far it goes is the fact under
  // test. Across the page, a drag to the right this long turns back to question 1; inside the
  // chat's sheet it is the chat's, and turns nothing.
  const log = chat.getByRole('log', 'Messages');
  const box = await log.boundingBox();
  if (!box) throw new Error('no chat log to drag in');
  const y = Math.round(box.y + Math.min(box.height / 2, 40));
  await screen.swipe({ from: { x: Math.round(box.x + 40), y }, to: { x: Math.round(box.x + 280), y } });
  await expect(chat).toBeVisible();
  await expect(screen.getByRole('region', 'Question 1 · answered')).toBeHidden();

  await chat.getByRole('button', 'Close the conversation').tap();
  await expect(question).toBeVisible();
  await expect(screen.getByRole('region', 'Question 1 · answered')).toBeHidden();
});
