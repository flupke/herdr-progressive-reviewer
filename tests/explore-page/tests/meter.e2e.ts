import { expect, type Locator } from 'e2e';
import { richTest, test } from './session.ts';

// The meter on the masthead's hairline says how much of the change the review marks cover. The
// fixture's change has three changed lines, one of which Jev marked when the round started; an
// answer to the first question marks the other two. The meter's name carries its numbers for a
// screen reader, and its window shows them.

// What the reviewer calls the meter: the line under the masthead, which names itself. The
// first question opens on the design screen, which the agent reads past before it answers: a
// run that records the steps takes longer than the usual 30 seconds.
const METER = {
  agentContext:
    'The meter is the line under the masthead, a button named "Lines reviewed: …"; a click on it pins its details open.',
  timeout: 90_000,
};

test('an answer advances the reviewed share of the change', METER, async ({ explore, screen, agent }) => {
  await explore.open();
  await explore.askQuestion();
  const meter = screen.getByRole('button', /^Lines reviewed/);
  await expect(meter).toHaveAccessibleName('Lines reviewed: 33%, 1 of 3 changed lines');
  await agent.act('open the details of the "Lines reviewed" meter');
  const details = screen.getByRole('group', 'Review marks of the change');
  await expect(details.getByText('33% reviewed')).toBeVisible();
  await expect(details.getByText('This question marks when you answer')).toBeVisible();

  await agent.act('answer question 1 with "Keep the draft" and send it');
  await expect(meter).toHaveAccessibleName('Lines reviewed: 100%, 3 of 3 changed lines');
  await agent.act('open the details of the "Lines reviewed" meter again');
  await expect(details.getByText('100% reviewed')).toBeVisible();
  await expect(details.getByText('Your answers · Q1')).toBeVisible();
});

test("the meter's window opens and closes with the keyboard", async ({ explore, screen }) => {
  await explore.open();
  await explore.askQuestion();
  const meter = screen.getByRole('button', /^Lines reviewed/);
  const details = screen.getByRole('group', 'Review marks of the change');
  await expect(details).toBeHidden();

  // The keys themselves are what this test checks, so it presses them rather than asking the
  // agent, which could open the window with the pointer.
  await meter.focus();
  await meter.press('Enter');
  await expect(meter).toBeExpanded();
  await expect(details.getByText('src/drafts.rs')).toBeVisible();

  await meter.press('Escape');
  await expect(details).toBeHidden();
  await expect(meter).toBeFocused();
});

test('a mark by hand during a round reaches the meter at once', async ({ explore, screen }) => {
  await explore.open();
  await explore.askQuestion();
  const meter = screen.getByRole('button', /^Lines reviewed/);
  await expect(meter).toHaveAccessibleName('Lines reviewed: 33%, 1 of 3 changed lines');

  await explore.markByHand();
  await expect(meter).toHaveAccessibleName('Lines reviewed: 100%, 3 of 3 changed lines');
});

test('the start cover says how large the change is', async ({ explore, screen }) => {
  await explore.open();
  await explore.reset();
  await expect(screen.getByText(/\+2 −1 in 1 file$/)).toBeVisible();
});

test('the meter keeps the same target when its bar grows, and the grown bar is opaque', async ({
  explore,
  screen,
  browser,
}) => {
  await explore.open();
  await explore.askQuestion();
  await explore.markByHand();
  const meter = screen.getByRole('button', /^Lines reviewed/);
  const atRest = await meter.boundingBox();

  // Exact keys open the window, which grows the bar, with no pointer to move it.
  await meter.focus();
  await meter.press('Enter');
  await expect(meter).toBeExpanded();
  // The target the pointer rests on is the same box at rest and grown, so growing the bar never
  // moves the pointer out of it.
  expect(await meter.boundingBox()).toEqual(atRest);
  // No locator reaches a computed colour: the page reads them, once the bar's colours have
  // finished their short transition. Every part of the grown bar is opaque, so the masthead's
  // hairline under it does not show through.
  await expect
    .poll(() =>
      browser.evaluate(() => {
        const colours = [...document.querySelectorAll('.meter.grown .meter-bar, .meter.grown .meter-segment:not(.pending)')]
          .filter((element) => element.getBoundingClientRect().width > 0)
          .map((element) => getComputedStyle(element).backgroundColor);
        // A colour with an alpha under 1 is an "rgba(…)", or ends with "/ 0.x)" or "/ 0)".
        const translucent = /^rgba\(|\/\s*(?:0?\.\d+|0)\s*\)$/;
        return colours.length > 1 && colours.every((colour) => !translucent.test(colour));
      }),
    )
    .toBe(true);
});

// The rich round's change has 43 files, most of them under one deep directory, one with a name
// longer than a row has room for beside its directory, and one under `.github/`.
const LONG_PATH = 'src/notify/delivery/batching/replies_sent_after_the_pane_closed_join_the_last_batch.rs';

richTest('the meter shows the name of every file of a large change', async ({ explore, screen, browser }) => {
  await explore.open();
  await explore.askQuestion();
  const meter = screen.getByRole('button', /^Lines reviewed/);
  const details = screen.getByRole('group', 'Review marks of the change');
  // A window 2000 pixels wide gives the paths all the room the window may take; one 1280 wide
  // gives it less than that, beside the reading column.
  for (const { wide, ...viewport } of [
    { width: 2000, height: 900, wide: true },
    { width: 1280, height: 800, wide: false },
  ]) {
    await browser.setViewport(viewport);
    // Exact keys open the window where the keyboard opens it, at the start of the column.
    await meter.focus();
    await meter.press('Enter');
    await expect(meter).toBeExpanded();
    await expect(details.getByRole('listitem', LONG_PATH)).toBeVisible();

    // The window grows past the handoff's 520 pixels for the long paths, within the viewport,
    // and its list of files scrolls inside it rather than running off the bottom. No locator
    // matcher reaches a box's size or place: the test reads the boxes. The window took its
    // width as it opened, and keeps it; its rise as it fades in only lifts it.
    const box = await details.boundingBox();
    if (!box) throw new Error('the window has no box');
    if (wide) expect(box.width).toBeGreaterThan(520);
    expect(box.x).toBeGreaterThanOrEqual(0);
    expect(box.x + box.width).toBeLessThanOrEqual(viewport.width);
    expect(box.y + box.height).toBeLessThanOrEqual(viewport.height);
    // The files do not fit either viewport's height: the window takes it, down to the page's
    // gutter, rather than stopping short of it.
    expect(box.y + box.height).toBeGreaterThan(viewport.height - 40);
    if (wide) await expectNamesWhole(details);

    await meter.press('Escape');
    await expect(meter).not.toBeExpanded();
  }
});

/**
 * Every row's file name lies whole inside its row: a path that does not fit gives up the start
 * of its directory, never the name. A cut name would run out of the row's box.
 */
async function expectNamesWhole(details: Locator): Promise<void> {
  const rows = await details.getByRole('listitem').all();
  expect(rows.length).toBeGreaterThan(0);
  const cut = [];
  for (const row of rows) {
    const path = (await row.getAttribute('aria-label')) ?? '';
    const rowBox = await row.boundingBox();
    const nameBox = await row.getByText(path.slice(path.lastIndexOf('/') + 1)).boundingBox();
    const inside =
      rowBox !== null &&
      nameBox !== null &&
      nameBox.x >= rowBox.x &&
      nameBox.x + nameBox.width <= rowBox.x + rowBox.width;
    if (!inside) cut.push(path);
  }
  expect(cut).toEqual([]);
}
