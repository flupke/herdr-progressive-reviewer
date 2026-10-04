// The Explore page's client: it opens the page's socket, draws each view the review tool sends,
// and sends the reviewer's actions back (docs/development.md, "The page's client").

/** @import { Seq, StateParams } from "./types.ts" */

import { Actions } from './actions.js';
import { showConnection } from './connection.js';
import { reloadOnChange } from './dev.js';
import { configureDiagrams } from './diagrams.js';
import { Page } from './page.js';
import { Link } from './socket.js';

const main = /** @type {HTMLElement} */ (document.getElementById('round'));
const line = /** @type {HTMLElement} */ (document.getElementById('connection'));
const page = new Page(main);
/** The epoch and number of the view the page shows. @type {{ epoch: string, seq: Seq }} */
let shown = { epoch: '', seq: { revision: -1, picks: -1 } };

/** Whether the view numbered `seq` comes after the one numbered `than`.
 * @param {Seq} seq
 * @param {Seq} than */
function after(seq, than) {
  return seq.revision > than.revision || (seq.revision === than.revision && seq.picks > than.picks);
}

const link = new Link({
  state: (/** @type {StateParams} */ params) => {
    const newer = params.epoch !== shown.epoch || after(params.seq, shown.seq);
    if (!newer) return;
    // A notice stays until the round changes; the view an action changed comes before its reply.
    page.notice = null;
    shown = { epoch: params.epoch, seq: params.seq };
    configureDiagrams(params.view.mermaid, (error) => {
      link.call({ method: 'diagram-failed', params: error }).catch(() => {
        // The tool is gone; the page still shows the error.
      });
    });
    page.render(params.view);
    main.dataset.seq = `${params.seq.revision}.${params.seq.picks}`;
    actions.enable();
  },
  link: (state) => {
    // A new socket starts from the round as it is now, whatever the number of its first view:
    // a restarted tool numbers its views from 1 again.
    if (state === 'open') shown = { epoch: '', seq: { revision: -1, picks: -1 } };
    showConnection(line, state);
    actions.connection(state === 'open');
  },
});
const actions = new Actions(main, link, page);
link.start();

const dev = main.dataset.dev;
if (dev) reloadOnChange(Number(dev));
