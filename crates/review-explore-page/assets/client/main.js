// The Explore page's client: it opens the page's socket, draws each view the review tool sends,
// and sends the reviewer's actions back (.agents/wiki/explore-page-client.md).

/** @import { Seq, StateParams } from "./types.ts" */

import { Actions } from './actions.js';
import { showConnection } from './connection.js';
import { reloadOnChange } from './dev.js';
import { configureDiagrams } from './diagrams.js';
import { Page } from './page.js';
import { leavePast, pastRound } from './route.js';
import { Link } from './socket.js';

const main = /** @type {HTMLElement} */ (document.getElementById('round'));
const line = /** @type {HTMLElement} */ (document.getElementById('connection'));
const header = /** @type {HTMLElement} */ (document.querySelector('header.masthead'));
// The page's own requests (the chat marks replies read) go on the socket, opened below.
const page = new Page(main, header, (call) => link.call(call));
/** The number before any view. @type {Seq} */
const NONE_SHOWN = { revision: -1, picks: -1, threads: -1, rounds: -1 };
/** The epoch and number of the view the page shows. @type {{ epoch: string, seq: Seq }} */
let shown = { epoch: '', seq: NONE_SHOWN };

/** Whether the view numbered `seq` comes after the one numbered `than`.
 * @param {Seq} seq
 * @param {Seq} than */
function after(seq, than) {
  if (seq.revision !== than.revision) return seq.revision > than.revision;
  if (seq.picks !== than.picks) return seq.picks > than.picks;
  if (seq.threads !== than.threads) return seq.threads > than.threads;
  return seq.rounds > than.rounds;
}

const link = new Link({
  state: (/** @type {StateParams} */ params) => {
    const newer = params.epoch !== shown.epoch || after(params.seq, shown.seq);
    if (!newer) return;
    // A notice stays until the round changes; the view an action changed comes before its reply.
    page.notice = null;
    shown = { epoch: params.epoch, seq: params.seq };
    // A past round this page cannot open (none has that identity, or the page is on the
    // network) leaves the page on the round it follows.
    if (link.past !== null && params.view.past === null) {
      leavePast();
      link.pastRefused();
    }
    configureDiagrams(params.view.mermaid, (error) => {
      link.call({ method: 'diagram-failed', params: error }).catch(() => {
        // The tool is gone; the page still shows the error.
      });
    });
    page.render(params.view);
    main.dataset.seq = `${params.seq.revision}.${params.seq.picks}.${params.seq.threads}.${params.seq.rounds}`;
    actions.enable();
  },
  link: (state) => {
    // A new socket starts from the round as it is now, whatever the number of its first view:
    // a restarted tool numbers its views from 1 again.
    if (state === 'open') shown = { epoch: '', seq: NONE_SHOWN };
    showConnection(line, state);
    actions.connection(state === 'open');
  },
}, pastRound());
const actions = new Actions(main, link, page);
link.start();
// The address names the round the page shows: a past one, or the one the page follows.
addEventListener('hashchange', () => link.follow(pastRound()));

const dev = main.dataset.dev;
if (dev) reloadOnChange(Number(dev));
