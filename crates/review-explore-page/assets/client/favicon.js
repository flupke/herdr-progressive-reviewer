// The browser tab's icon tells a reviewer who left the tab where the round stands
// (docs/logo/README.md, "Favicon states"), beside the tab's title (masthead.js):
//
// - At rest, the mark's meter shows the share of changed lines reviewed, in quarters: empty, 1/4,
//   1/2, 3/4, and full only once every changed line is reviewed, never rounded up to it; any
//   reviewed line shows at least a quarter. It is the meter's green bar (meter.js), in small.
// - While the agent works, the meter becomes the page's progress runner: a blue segment slides
//   along the track, four frames in a loop. Under prefers-reduced-motion it stays still.
// - Without a round, or a change to measure, the tab shows the plain mark (favicon.svg).
//
// The colours are the page's tokens (tokens.css): --panel, --good, --line on --panel, --accent,
// each in its dark and light value; the icon follows the browser's colour scheme by itself.

/** @import { PageView } from "./types.ts" */

import { keyOf } from './dom.js';
import { measured } from './meter.js';

// The classes of the icon's parts: the tile, the check, the green fill, the track and the blue
// runner.
const STYLE =
  '.t{fill:#1b1d21}.c{stroke:#3fb950}.g{fill:#3fb950}.k{fill:#3a3d42}.a{fill:#4493f8}' +
  '@media (prefers-color-scheme:light){.t{fill:#f5f7f9;stroke:#d2d4d7;stroke-width:1}' +
  '.c{stroke:#1a7f37}.g{fill:#1a7f37}.k{fill:#d2d4d7}.a{fill:#0969da}}';

/** The bar under the check, on the icon's 32-unit grid. */
const BAR = { x: 4, y: 21.5, w: 24, h: 6 };
/** The runner's visible span in each busy frame, as [x, width]: it enters at the left and leaves
 * at the right, like the page's indeterminate bar. */
const RUNNER = [
  [4, 6],
  [8.5, 8],
  [15.5, 8],
  [22, 6],
];
/** How long each busy frame shows, in milliseconds. */
const FRAME_MS = 400;

/** What the icon shows. @typedef {{ kind: 'mark' } | { kind: 'progress', step: number } | { kind: 'busy' }} Shown */

export class Favicon {
  constructor() {
    const link = document.querySelector('link[rel="icon"][type="image/svg+xml"]');
    if (!(link instanceof HTMLLinkElement)) throw new Error('the page names no SVG icon');
    this.link = link;
    /** The plain mark, which the page's shell names. */
    this.mark = link.href;
    /** @type {Shown} */
    this.shown = { kind: 'mark' };
    this.frame = 0;
    this.reducedMotion = matchMedia('(prefers-reduced-motion: reduce)');
    this.reducedMotion.addEventListener('change', () => this.render());
    this.ticker = new Ticker(() => {
      this.frame = (this.frame + 1) % RUNNER.length;
      this.show(busySvg(this.frame));
    });
  }

  /** @param {PageView} view */
  update(view) {
    const tally = measured(view);
    /** @type {Shown} */
    let shown = { kind: 'mark' };
    // The agent works while the tab's title says so (masthead.js).
    if (view.title?.kind === 'agent_working') shown = { kind: 'busy' };
    else if (tally) shown = { kind: 'progress', step: progressStep(tally.change.share.marked, tally.change.share.changed) };
    if (keyOf(shown) === keyOf(this.shown)) return;
    this.shown = shown;
    this.render();
  }

  render() {
    this.ticker.stop();
    switch (this.shown.kind) {
      case 'mark':
        this.link.href = this.mark;
        return;
      case 'progress':
        this.show(progressSvg(this.shown.step));
        return;
      case 'busy':
        // Without motion, the runner stays in its second frame, mid-track, which reads as work
        // under way rather than as a quarter of the bar.
        this.frame = this.reducedMotion.matches ? 1 : 0;
        this.show(busySvg(this.frame));
        if (!this.reducedMotion.matches) this.ticker.start(FRAME_MS);
    }
  }

  /** @param {string} svg */
  show(svg) {
    this.link.href = `data:image/svg+xml,${encodeURIComponent(svg)}`;
  }
}

/**
 * The quarters of the bar that `marked` of `changed` lines fill: 4 only once every line is
 * marked, and at least 1 once any is.
 * @param {number} marked
 * @param {number} changed
 */
function progressStep(marked, changed) {
  if (changed <= 0 || marked <= 0) return 0;
  if (marked >= changed) return 4;
  return Math.min(3, Math.max(1, Math.round((marked / changed) * 4)));
}

/** @param {string} bar the bar's fill, drawn over its track */
function icon(bar) {
  return (
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32">' +
    `<style>${STYLE}</style>` +
    '<rect class="t" x=".5" y=".5" width="31" height="31" rx="6"/>' +
    '<path class="c" d="M8 11.6 L12.6 16 L23.6 5.4" fill="none" stroke-width="4.2" ' +
    'stroke-linecap="round" stroke-linejoin="round"/>' +
    `<rect class="k" x="${BAR.x}" y="${BAR.y}" width="${BAR.w}" height="${BAR.h}" rx="3"/>` +
    bar +
    '</svg>'
  );
}

/** The icon at rest, `step` quarters of its bar in green. @param {number} step */
function progressSvg(step) {
  const width = (BAR.w * step) / 4;
  return icon(width ? `<rect class="g" x="${BAR.x}" y="${BAR.y}" width="${width}" height="${BAR.h}" rx="3"/>` : '');
}

/** One frame of the busy icon, 0 to 3. @param {number} frame */
function busySvg(frame) {
  const [x, width] = RUNNER[frame];
  return icon(`<rect class="a" x="${x}" y="${BAR.y}" width="${width}" height="${BAR.h}" rx="3"/>`);
}

/**
 * A repeating timer that keeps going in a hidden tab: a browser throttles a hidden page's own
 * timers to one a second, and Chrome to one a minute after five minutes, but a worker's less
 * (favicon-ticker.js). Where no worker starts, the page's own timer runs it.
 */
class Ticker {
  /** @param {() => void} tick */
  constructor(tick) {
    this.tick = tick;
    /** @type {ReturnType<typeof setInterval> | undefined} */
    this.timer = undefined;
    /** The period of the ticks, in milliseconds, or 0 while stopped. */
    this.period = 0;
    /** @type {Worker | null} */
    this.worker = null;
    try {
      const worker = new Worker('/assets/client/favicon-ticker.js', { type: 'module' });
      // A tick the worker sent before it heard the stop draws nothing.
      worker.onmessage = () => {
        if (this.period > 0) this.tick();
      };
      // A worker that cannot load says so here, not as it is created.
      worker.onerror = () => {
        this.worker = null;
        if (this.period > 0) this.start(this.period);
      };
      this.worker = worker;
    } catch {
      this.worker = null;
    }
  }

  /** @param {number} ms */
  start(ms) {
    this.stop();
    this.period = ms;
    if (this.worker) this.worker.postMessage(ms);
    else this.timer = setInterval(this.tick, ms);
  }

  stop() {
    this.period = 0;
    this.worker?.postMessage(0);
    clearInterval(this.timer);
  }
}
