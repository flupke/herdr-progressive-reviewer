import { expect } from 'e2e';
import { test } from './session.ts';

// The browser tab's icon tells a reviewer who left the tab where the round stands: the logo's
// mark before a round, then its bar as the meter in quarters, and the blue runner while the agent
// works. The fixture's change has three changed lines, one of which Jev marked when the round
// started; an answer to the first question marks the other two.

/** The review of the fixture's session, which the tab title names. */
const REVIEW = "Keep the reviewer's draft when a round reopens";

/**
 * What the tab's icon shows: `mark` for the page's own icon, `busy` for the runner, or the
 * quarters of the bar in green, such as `1/4`. The icon is a `<link>` in the page's head, which
 * no locator reaches: it is read in the page.
 */
async function tabIcon(browser: { evaluate<T>(read: () => T): Promise<T> }) {
  return browser.evaluate(() => {
    const href = document.querySelector<HTMLLinkElement>('link[rel="icon"][type="image/svg+xml"]')?.href ?? '';
    if (!href.startsWith('data:')) return new URL(href).pathname === '/assets/favicon.svg' ? 'mark' : href;
    const svg = decodeURIComponent(href.slice(href.indexOf(',') + 1));
    if (svg.includes('class="a"')) return 'busy';
    const width = (part: string) => Number(new RegExp(`class="${part}"[^>]*width="([\\d.]+)"`).exec(svg)?.[1] ?? 0);
    // The green fill over the track, in quarters of the track.
    return `${Math.round((width('g') / width('k')) * 4)}/4`;
  });
}

test('the tab icon follows the reviewed share and the agent at work', async ({ explore, browser }) => {
  // The fixture's round starts with the agent preparing the first question.
  await explore.open();
  await expect.poll(() => tabIcon(browser)).toBe('busy');

  await explore.askQuestion();
  await expect(browser).toHaveTitle(`Q1 · your turn — ${REVIEW}`);
  // One line of three is a third, which shows as a quarter.
  await expect.poll(() => tabIcon(browser)).toBe('1/4');

  await explore.answerInPane();
  await expect(browser).toHaveTitle(`Agent working… — ${REVIEW}`);
  await expect.poll(() => tabIcon(browser)).toBe('busy');

  await explore.askQuestion();
  await expect(browser).toHaveTitle(`Q2 · your turn — ${REVIEW}`);
  await expect.poll(() => tabIcon(browser)).toBe('4/4');
});

test('without a round, the tab shows the plain mark', async ({ explore, screen, browser }) => {
  await explore.open();
  await explore.reset();
  await expect(screen.getByRole('button', 'Start')).toBeVisible();
  await expect.poll(() => tabIcon(browser)).toBe('mark');
});
