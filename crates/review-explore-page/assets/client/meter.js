// The meter on the masthead's hairline (docs/design/explore-page/README.md, "Meter"): how much
// of the change the review marks cover. At rest it is a 2-pixel green bar whose width is the
// share of the changed lines that are marked, and nothing else. Hovered or focused, it grows to
// 8 pixels and splits by who marked the lines: the reviewer's answers, marks by hand or from
// earlier rounds, Jev and not relevant, then what the question that waits marks once answered;
// a window under the pointer gives the numbers for the change and for each file. A click, or
// Enter on the focused meter, pins the window; Escape, or a click elsewhere, closes it. When
// marks grow the green edge advances, and it glows once when an answer applied its marks.
//
// The numbers come from the review tool (`PageView.tally`), which sends them with the round and
// again whenever the review marks change.

/** @import { MarkTally, PageView, RailStep, Tally } from "./types.ts" */

import { changeSize, fileCount } from './change-size.js';
import { h, keyOf, Region } from './dom.js';

/** How the window is open: under the pointer or the focus, pinned, or closed. */
/** @typedef {'hover' | 'pinned' | null} Open */

/** The segments of the grown bar, in order. */
const SEGMENTS = /** @type {const} */ (['answers', 'others', 'jev', 'pending']);

/** How long the window stays once the pointer left the meter, in milliseconds. */
const LEAVE_GRACE = 180;
/** The room the window keeps from the page's panel, in pixels. */
const PANEL_ROOM = 16;

export class Meter {
  /** @param {HTMLElement} line the masthead's hairline, which the meter draws on */
  constructor(line) {
    this.segments = Object.fromEntries(SEGMENTS.map((name) => [name, h('span', { class: `meter-segment ${name}` })]));
    this.glow = h('span', { class: 'meter-glow', 'aria-hidden': 'true' });
    this.strip = h('button', {
      class: 'meter-strip',
      type: 'button',
      'aria-expanded': 'false',
      'aria-controls': 'meter-window',
      onclick: () => this.toggle(),
      onmouseenter: (/** @type {Event} */ event) => this.enter(/** @type {MouseEvent} */ (event)),
      onmouseleave: () => {
        this.pointed = false;
        this.leave();
      },
      onfocus: () => {
        this.focused = this.strip.matches(':focus-visible');
        this.refresh();
      },
      onblur: () => {
        this.focused = false;
        this.refresh();
      },
    });
    this.window = h('div', {
      class: 'meter-window',
      id: 'meter-window',
      role: 'group',
      'aria-label': 'Review marks of the change',
      onmouseenter: () => this.enter(null),
      onmouseleave: () => this.leave(),
    });
    this.root = h(
      'div',
      { class: 'meter', hidden: true },
      h('div', { class: 'meter-bar', 'aria-hidden': 'true' }, SEGMENTS.map((name) => this.segments[name])),
      this.glow,
      this.strip,
      this.window,
    );
    line.append(this.root);
    this.content = new Region(this.window, 'meter-window');
    /** @type {Open} */
    this.open = null;
    /** Whether the pointer is on the strip. */
    this.pointed = false;
    /** Whether the keyboard's focus is on the strip. */
    this.focused = false;
    /** @type {ReturnType<typeof setTimeout> | undefined} */
    this.closing = undefined;
    /**
     * The marked lines, and the lines the waiting question marks, at the latest view: an answer
     * applied its marks when the first grows as the second falls to zero.
     * @type {{ marked: number, pending: number } | null}
     */
    this.previous = null;
    document.addEventListener('keydown', (event) => {
      if (event.key === 'Escape' && this.open) this.close();
    });
    document.addEventListener('click', (event) => {
      if (this.open === 'pinned' && !event.composedPath().includes(this.root)) this.close();
    });
  }

  /** @param {PageView} view */
  update(view) {
    const tally = view.tally;
    const shows = view.rail.length > 0 && tally !== null && tally.files.length > 0;
    this.root.hidden = !shows;
    if (!shows || !tally) {
      this.previous = null;
      if (this.open) this.close();
      return;
    }
    const change = tally.change;
    this.strip.setAttribute(
      'aria-label',
      `Lines reviewed: ${change.share.percent}%, ${lineShare(change.share.marked, change.share.changed)}`,
    );
    this.draw(change);
    this.content.show(keyOf([tally, view.rail]), () => windowContent(tally, answeredSteps(view.rail)));
  }

  /** Draws the bar's segments for `change`, and the glow when an answer applied its marks.
   * @param {Tally} change */
  draw(change) {
    const total = change.share.changed;
    const widths = segmentLines(change);
    let left = 0;
    for (const name of SEGMENTS) {
      const segment = this.segments[name];
      const share = total > 0 ? (widths[name] / total) * 100 : 0;
      if (name === 'pending' && share === 0 && segment.dataset.lines) {
        // The answer was sent: the hatched part fades where it was, under the advancing edge.
        segment.style.opacity = '0';
        delete segment.dataset.lines;
        continue;
      }
      segment.style.left = `${left}%`;
      segment.style.width = `${share}%`;
      segment.style.opacity = '';
      if (name === 'pending') {
        if (share > 0) segment.dataset.lines = String(widths[name]);
        else delete segment.dataset.lines;
      }
      left += share;
    }
    const done = total > 0 ? (change.share.marked / total) * 100 : 0;
    this.glow.style.width = `${done}%`;
    const now = { marked: change.share.marked, pending: widths.pending };
    if (this.previous && this.previous.pending > 0 && now.pending === 0 && now.marked > this.previous.marked) {
      // An answer applied its marks: the one-shot glow starts again.
      this.glow.classList.remove('glowing');
      void this.glow.offsetWidth;
      this.glow.classList.add('glowing');
    }
    this.previous = now;
  }

  /** The pointer is on the meter or its window: it grows, and the window opens under it.
   * @param {MouseEvent | null} pointer the pointer on the strip, or `null` on the window */
  enter(pointer) {
    clearTimeout(this.closing);
    if (pointer) {
      this.pointed = true;
      if (this.open !== 'pinned') this.place(pointer.clientX);
    }
    if (!this.open) this.show('hover');
    this.refresh();
  }

  leave() {
    this.refresh();
    if (this.open !== 'hover') return;
    clearTimeout(this.closing);
    this.closing = setTimeout(() => {
      if (this.open === 'hover') this.close();
    }, LEAVE_GRACE);
  }

  /** A click, or Enter or Space on the focused meter, pins the window open, or closes it. */
  toggle() {
    if (this.open === 'pinned') {
      this.close();
      return;
    }
    if (!this.open) this.place(null);
    this.show('pinned');
  }

  /** @param {Open} open */
  show(open) {
    this.open = open;
    this.refresh();
  }

  close() {
    clearTimeout(this.closing);
    this.open = null;
    this.refresh();
  }

  /** Grows the bar while the meter is pointed at, focused or open, and shows the window. */
  refresh() {
    this.root.classList.toggle('open', this.open !== null);
    this.root.classList.toggle('grown', this.open !== null || this.pointed || this.focused);
    this.strip.setAttribute('aria-expanded', String(this.open !== null));
  }

  /**
   * Places the window under the pointer at `x`, or at the start of the page's column for the
   * keyboard, within the masthead's gutters and never over the panel beside the reading
   * column.
   * @param {number | null} x
   */
  place(x) {
    const line = this.root.getBoundingClientRect();
    const header = this.root.closest('.masthead');
    const gutter = header ? parseFloat(getComputedStyle(header).paddingLeft) : 16;
    // The stylesheet sizes the window; it keeps its size while hidden.
    const width = this.window.offsetWidth;
    let right = line.width - gutter;
    const panel = document.querySelector('.screen:not([hidden]) .panel');
    if (panel instanceof HTMLElement) {
      const box = panel.getBoundingClientRect();
      // A panel beside the reading column, not under it as on a phone.
      if (box.left > line.left + width) right = Math.min(right, box.left - line.left - PANEL_ROOM);
    }
    const wanted = x === null ? gutter : x - line.left - width / 2;
    const left = Math.max(gutter, Math.min(wanted, right - width));
    this.window.style.left = `${left}px`;
  }
}

/**
 * The window's content: the totals, the legend with its counts, and one row for each file.
 * @param {MarkTally} tally
 * @param {number[]} answered the questions the reviewer answered, by their number on the rail
 */
function windowContent(tally, answered) {
  const change = tally.change;
  const marked = change.marked;
  const { answers, others, jev, pending } = segmentLines(change);
  const largest = Math.max(1, ...tally.files.map((file) => file.tally.share.changed));
  return [
    h(
      'div',
      { class: 'meter-totals' },
      h('span', { class: 'meter-share' }, `${change.share.percent}% reviewed`),
      h('span', { class: 'meter-lines' }, lineShare(change.share.marked, change.share.changed)),
      h('span', { class: 'meter-size' }, changeSize(tally), ` · ${fileCount(tally.files.length)}`),
    ),
    h(
      'div',
      { class: 'meter-legend' },
      legendRow('answers', answered.length > 0 ? `Your answers · ${answered.map((n) => `Q${n}`).join(', ')}` : 'Your answers', answers),
      others > 0 ? legendRow('others', othersLabel(marked.by_hand, marked.other_rounds), others) : null,
      legendRow('jev', `Jev at the start ${marked.jev} · not relevant ${marked.not_relevant}`, jev),
      pending > 0 ? legendRow('pending', 'This question marks when you answer', pending) : null,
      legendRow('left', 'Left to explore', change.left),
    ),
    h(
      'div',
      { class: 'meter-files' },
      tally.files.map((file) => {
        const size = file.tally.share.changed;
        const whole = file.whole;
        const state = whole ? (whole.marked_by ? 'marked whole' : 'left whole') : `${file.tally.left} left`;
        return [
          h('span', { class: 'meter-path', title: file.path }, file.path),
          whole ? h('span', {}) : fileBar(file.tally, (size / largest) * 100),
          h('span', { class: 'meter-file-state' }, state, file.cited ? [' · ', h('span', { class: 'cited' }, 'cited here')] : null),
        ];
      }),
    ),
  ];
}

/**
 * The legend's words for the lines marked by hand and in earlier rounds: "By hand 3 · earlier
 * rounds 2", each part only when it counts lines.
 * @param {number} byHand
 * @param {number} otherRounds
 */
function othersLabel(byHand, otherRounds) {
  const parts = [];
  if (byHand > 0) parts.push(`By hand ${byHand}`);
  if (otherRounds > 0) parts.push(`${parts.length > 0 ? 'earlier' : 'Earlier'} rounds ${otherRounds}`);
  return parts.join(' · ');
}

/**
 * One row of the legend: its swatch, its words and its count.
 * @param {string} kind
 * @param {string} label
 * @param {number} count
 */
function legendRow(kind, label, count) {
  return [h('span', { class: `meter-swatch ${kind}` }), h('span', {}, label), h('span', { class: 'meter-count' }, String(count))];
}

/**
 * A file's bar, `width` percent of the widest, with the segments of the meter.
 * @param {Tally} tally
 * @param {number} width
 */
function fileBar(tally, width) {
  const total = tally.share.changed;
  const parts = segmentLines(tally);
  const bar = h('span', { class: 'meter-file-bar' });
  bar.style.width = `${width}%`;
  for (const name of SEGMENTS) {
    if (parts[name] === 0) continue;
    const part = h('span', { class: `meter-swatch ${name}` });
    part.style.width = `${(parts[name] / total) * 100}%`;
    bar.append(part);
  }
  return bar;
}

/**
 * The lines of each segment of the meter: the reviewer's answers, by hand or in earlier rounds,
 * Jev and not relevant, and what the waiting question marks.
 * @param {Tally} tally
 * @returns {Record<typeof SEGMENTS[number], number>}
 */
function segmentLines(tally) {
  const marked = tally.marked;
  return {
    answers: marked.answers,
    others: marked.by_hand + marked.other_rounds,
    jev: marked.jev + marked.not_relevant,
    pending: tally.pending.reviewed + tally.pending.not_relevant,
  };
}

/** "52 of 135 changed lines". @param {number} marked @param {number} changed */
function lineShare(marked, changed) {
  return `${marked} of ${changed} changed ${changed === 1 ? 'line' : 'lines'}`;
}

/**
 * The questions the reviewer answered, by their number on the rail: the done ones, and the one
 * the agent works on the answer to.
 * @param {RailStep[]} rail
 */
function answeredSteps(rail) {
  return rail.flatMap((step) => {
    if (step.step.kind !== 'question') return [];
    const state = step.state;
    const answered = state.kind === 'done' || (state.kind === 'current' && state.working);
    return answered ? [step.step.number] : [];
  });
}
