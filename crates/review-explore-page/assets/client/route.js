// The page's addresses: the fragment of the address says which screen shows, so that a link
// opens a screen, the browser's Back returns to the one before, and a reload keeps it.
//
// - `#design` shows the design screen at its top, and `#design-part-N` at its part N (from 1):
//   the targets of the links that open the design (its own map, and the rail's "Design ▾" once
//   the masthead has one);
// - `#question-N` shows question N as the reviewer answered it, read only, while it is an earlier
//   question of the round: the target of its done step on the rail;
// - `#quiz-N` shows item N of the conclusion's quiz as the reviewer answered it, read only, once
//   the quiz is over: the target of its done step on the rail and of the score beside the
//   conclusion (while the quiz asks, the item it shows is the page's own state, page.js);
// - `#past-rounds` opens the ⋯ menu on the list of past rounds, over the round's current stage:
//   the address at which the pane's "Past rounds on the Explore page" opens the page on this
//   machine (`PageScreen::PastRounds` in crates/review-explore-page-opening); once the list
//   shows, the address names the stage;
// - any other fragment, or none, shows the round's current stage; `#round` names it, for a link
//   back from the design.
//
// A past round, which only the page on this machine opens from its ⋯ menu, adds its identity to
// each of these: `#round@<id>` shows its stage, `#design@<id>` its design, `#question-2@<id>` its
// second question. While a past round shows, a link of the page to one of its screens keeps the
// identity (a link marked `data-leave-past` leaves the past round), and the page opens the
// round's socket again for the round the address names (main.js).
//
// The design opens the round: the first time a tab shows a round that waits for the answer to
// its first question, the page shows the design screen, whatever the address said before.

/** @import { PageView } from "./types.ts" */

/** The design screen, at its top. */
export const DESIGN = '#design';
/** The round's current stage. */
export const STAGE = '#round';
/** The list of past rounds, in the ⋯ menu. */
export const PAST_ROUNDS = '#past-rounds';

const PART = /^#design-part-([1-9][0-9]*)$/;
/** The identity of a past round at the end of an address. */
const PAST = /@([A-Za-z0-9-]+)$/;
const QUESTION = /^#question-([1-9][0-9]*)$/;
const QUIZ = /^#quiz-([1-9][0-9]*)$/;

/** The address of part `number` of the design screen.
 * @param {number} number */
export function designPart(number) {
  return `#design-part-${number}`;
}

/** The address of earlier question `number`, as the reviewer answered it.
 * @param {number} number */
export function earlierQuestion(number) {
  return `#question-${number}`;
}

/** The address of item `number` (from 1) of the answered quiz.
 * @param {number} number */
export function quizItem(number) {
  return `#quiz-${number}`;
}

/**
 * The screen the address asks for: the design, at one of its parts or at its top, an earlier
 * question, an item of the answered quiz, or the stage.
 * @typedef {{ screen: 'design', part: number | null } | { screen: 'question', number: number }
 *   | { screen: 'quiz', item: number } | { screen: 'stage' }} Route
 * @returns {Route} */
export function route() {
  const hash = location.hash.replace(PAST, '');
  if (hash === DESIGN) return { screen: 'design', part: null };
  const part = PART.exec(hash);
  if (part) return { screen: 'design', part: Number(part[1]) };
  const question = QUESTION.exec(hash);
  if (question) return { screen: 'question', number: Number(question[1]) };
  const quiz = QUIZ.exec(hash);
  return quiz ? { screen: 'quiz', item: Number(quiz[1]) } : { screen: 'stage' };
}

/** The past round the address names, by its identity; `null` for the round the page follows. */
export function pastRound() {
  return PAST.exec(location.hash)?.[1] ?? null;
}

/** `address`, one of the page's screens, in the past round `past`, or as it is with `null`.
 * @param {string} address
 * @param {string | null} past */
function inPast(address, past) {
  return past === null ? address : `${address}@${past}`;
}

/** The address of the stage of the past round `id`.
 * @param {string} id */
export function pastAddress(id) {
  return inPast(STAGE, id);
}

/** `address`, one of the page's screens, in the past round the address names, if any.
 * @param {string} address */
export function here(address) {
  return inPast(address, pastRound());
}

/** The address without the past round it names: the page follows its round again. */
export function leavePast() {
  history.replaceState(history.state, '', location.hash.replace(PAST, '') || STAGE);
}

// While a past round shows, a link to a screen of the page opens that screen of the past round.
document.addEventListener('click', (event) => {
  const past = pastRound();
  if (past === null || event.defaultPrevented || !(event.target instanceof Element)) return;
  const link = event.target.closest('a[href^="#"]');
  if (!(link instanceof HTMLAnchorElement) || link.dataset.leavePast !== undefined) return;
  const address = link.getAttribute('href') ?? '';
  if (PAST.test(address)) return;
  event.preventDefault();
  location.hash = inPast(address, past);
});

const storage = (() => {
  try {
    return window.sessionStorage;
  } catch {
    return null;
  }
})();
/** The rounds this page opened, for a browser that keeps nothing in its storage. */
const opened = new Set();
/** The round whose design the page showed by itself, while it shows it. @type {string | null} */
let shownBySelf = null;

/**
 * Shows the design screen if `view` is the opening of a round this tab has not opened yet: the
 * round waits for the answer to its first question, and the agent explained its design. A round
 * is opened once per tab, so that a reload or the way back from the design does not bring the
 * design back. A design the page showed by itself gives way to the round's stage once the round
 * moves on (answered in the pane, say); one the reviewer opened stays.
 * @param {PageView} view
 * @param {number | null} waitingFor the number of the question the round waits for, if any
 */
export function openRound(view, waitingFor) {
  const round = view.question?.round ?? null;
  // A past round opens where the reviewer asked.
  const opening = view.design !== null && round !== null && waitingFor === 1 && view.past === null;
  if (shownBySelf !== null && !(opening && round === shownBySelf)) {
    if (location.hash === DESIGN) history.replaceState(history.state, '', STAGE);
    shownBySelf = null;
  }
  if (!opening || round === null || opened.has(round)) return;
  opened.add(round);
  const name = `explore-opened:${round}`;
  try {
    if (storage?.getItem(name)) return;
    storage?.setItem(name, '1');
  } catch {
    // The browser keeps nothing for this page: the round opens on its design at each reload.
  }
  if (route().screen !== 'design') history.replaceState(history.state, '', DESIGN);
  shownBySelf = round;
}

addEventListener('hashchange', () => {
  // The reviewer went somewhere: a design they open now is theirs.
  shownBySelf = null;
});
