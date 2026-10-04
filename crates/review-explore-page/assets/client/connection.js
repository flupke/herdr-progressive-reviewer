// The quiet line above the page that says when the page cannot reach the review tool: while it
// reconnects, the page's actions wait; once its address opens no round any more, it says how to
// open the round again.

/** @import { LinkState } from "./socket.js" */

const WORDS = {
  connecting: null,
  open: null,
  down: 'Reconnecting to the review tool… Your actions wait until it is back.',
  ended:
    'This address does not open an Explore round any more. Open the page again from the pane: its QR code, or the Herdr action that opens it.',
};

/**
 * @param {HTMLElement} line
 * @param {LinkState} state
 */
export function showConnection(line, state) {
  const words = WORDS[state];
  line.hidden = words === null;
  line.textContent = words ?? '';
  line.classList.toggle('ended', state === 'ended');
}
