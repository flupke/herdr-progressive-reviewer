// The agent's notifications: the reviewer turns them on in the ⋯ menu, and the browser tells them
// when the agent finishes while they look elsewhere. The browser's `Notification` is replaced by
// one that records what the page shows, and the page reports that it has no focus.
import { expect } from 'e2e';
import { test } from './session.ts';

test('the reviewer turns on notifications and hears when the next question is ready and when the agent replies', async ({ explore, screen, browser }) => {
  await explore.open();
  await explore.askQuestion();
  await browser.evaluate(() => {
    const page = window as unknown as { shown: string[]; Notification: unknown };
    page.shown = [];
    page.Notification = class {
      static permission = 'default';
      static async requestPermission() {
        this.permission = 'granted';
        return 'granted';
      }
      onclick: unknown = null;
      constructor(title: string) {
        page.shown.push(title);
      }
      close() {}
    };
    document.hasFocus = () => false;
  });
  const shown = () => browser.evaluate(() => (window as unknown as { shown: string[] }).shown);

  await screen.getByRole('button', 'Round menu').tap();
  const notify = screen.getByRole('button', /^Notify me when the agent finishes/);
  await expect(notify).toHaveAttribute('aria-pressed', 'false');
  await notify.tap();
  await expect(notify).toHaveAttribute('aria-pressed', 'true');

  await explore.answerInPane();
  await expect(browser).toHaveTitle(/^Agent working/);
  expect(await shown()).toEqual([]);
  await explore.askQuestion();
  // The page notifies in the same drawing that retitles the tab.
  await expect(browser).toHaveTitle(/^Q2 · your turn/);
  expect(await shown()).toEqual(['Question 2 is ready']);

  // A reply in the chat, which stays open, notifies too: the reviewer is in another window.
  await screen.getByRole('button', 'Talk to the agent').tap();
  const chat = screen.getByRole('complementary', 'Conversation with the agent');
  await chat.getByRole('textbox', 'Message to the agent').fill('Why?');
  await chat.getByRole('button', 'Send').tap();
  await explore.agentReplies();
  await expect(chat.getByRole('article', 'Reply from the agent')).toBeVisible();
  expect(await shown()).toEqual(['Question 2 is ready', 'The agent replied in the chat']);
});
