// The page's addresses: the fragment of the address says which screen shows, so that a link
// opens a screen, the browser's Back returns to the one before, and a reload keeps it.
//
// - `#design` shows the design screen at its top, and `#design-part-N` at its part N (from 1):
//   the targets of the links that open the design (its own map, and the rail's "Design ▾" once
//   the masthead has one);
// - `#question-N` shows question N as the reviewer answered it, read only, while it is an earlier
//   question of the round: the target of its done step on the rail;
// - any other fragment, or none, shows the round's current stage; `#round` names it, for a link
//   back from the design.
//
// The design opens the round: the first time a tab shows a round that waits for the answer to
// its first question, the page shows the design screen, whatever the address said before.

/** @import { PageView } from "./types.ts" */

/** The design screen, at its top. */
export const DESIGN = '#design';
/** The round's current stage. */
export const STAGE = '#round';

const PART = /^#design-part-([1-9][0-9]*)$/;
const QUESTION = /^#question-([1-9][0-9]*)$/;

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

/**
 * The screen the address asks for: the design, at one of its parts or at its top, an earlier
 * question, or the stage.
 * @typedef {{ screen: 'design', part: number | null } | { screen: 'question', number: number }
 *   | { screen: 'stage' }} Route
 * @returns {Route} */
export function route() {
  const hash = location.hash;
  if (hash === DESIGN) return { screen: 'design', part: null };
  const part = PART.exec(hash);
  if (part) return { screen: 'design', part: Number(part[1]) };
  const question = QUESTION.exec(hash);
  return question ? { screen: 'question', number: Number(question[1]) } : { screen: 'stage' };
}

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
  const opening = view.design !== null && round !== null && waitingFor === 1;
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
