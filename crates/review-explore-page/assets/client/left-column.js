// The column at the window's left (left-column.css): the chat (chat.js) or the list of past
// rounds (past.js), one at a time, so that opening one closes the other. On a desktop it stands
// under the masthead: beside the reading column from 89rem, where the page makes room for it
// (layout.css), over the left of the page below that; below the desk's two columns it is a
// bottom sheet over the dimmed page, whose grabber drags it to the window's full height and back,
// or down to close it. It slides in as it opens, fades out as it closes, rises with the masthead as
// the page scrolls, and Escape, its × and the dimmed page close it.

import { DESK } from './desk.js';
import { h } from './dom.js';

/** The event a column sends as it opens, which closes the other. */
const OPENED = 'explore:left-column';

/** How far a drag of the phone sheet's grabber goes before it changes the sheet. */
const DRAG = 60;

/** The page's token `name` (tokens.css, layout.css) as a number of pixels or milliseconds.
 * @param {string} name */
function token(name) {
  return parseFloat(getComputedStyle(document.documentElement).getPropertyValue(name)) || 0;
}

/** Whether the browser lets the column rise with the masthead by itself (left-column.css). */
const RISES = CSS.supports('animation-timeline: scroll()');

export class LeftColumn {
  /**
   * @param {object} parts
   * @param {'aside' | 'nav'} parts.tag the column's element
   * @param {string} parts.kind its class beside `left-column`, which is also its identity
   * @param {string} parts.label its accessible name
   * @param {string} parts.title the title of its head
   * @param {string} parts.closeLabel the accessible name of its ×
   * @param {(refocus: boolean) => void} parts.closed called once it closed; `refocus` when the
   *   focus goes back to what opened it
   */
  constructor({ tag, kind, label, title, closeLabel, closed }) {
    this.closed = closed;
    this.open = false;
    this.element = h(tag, { class: `left-column ${kind}`, id: kind, 'aria-label': label, hidden: true });
    /** What the head says after the title. */
    this.meta = h('span', { class: 'left-meta' });
    const grabber = h('div', { class: 'left-grabber', 'aria-hidden': 'true' });
    this.dragSheet(grabber);
    this.element.append(
      grabber,
      h(
        'header',
        { class: 'left-head' },
        h('h2', { class: 'left-title' }, title),
        this.meta,
        h('button', { class: 'left-close', type: 'button', 'aria-label': closeLabel, onclick: () => this.close(true) }, '×'),
      ),
    );
    this.scrim = h('div', { class: 'left-scrim', hidden: true, onclick: () => this.close(false) });
    document.body.append(this.scrim, this.element);
    document.addEventListener(OPENED, (event) => {
      if (/** @type {CustomEvent<LeftColumn>} */ (event).detail !== this) this.close(false);
    });
    // On the window, which hears a key after every listener of the document: an Escape that
    // closed something else (a popover of the masthead, the meter's pinned window, a diagram
    // opened large) leaves the column open.
    addEventListener('keydown', (event) => {
      if (event.key !== 'Escape' || !this.open || event.defaultPrevented) return;
      if (event.target instanceof Element && event.target.closest('dialog')) return;
      this.close(true);
    });
    if (!RISES) {
      addEventListener('scroll', () => this.rise(), { passive: true });
      addEventListener('resize', () => this.rise());
    }
  }

  /** Where the browser has no scroll timeline: on a desktop, the column rises with the masthead
   * as the page scrolls, to where the panel is held, as `left-column-rise` in left-column.css
   * does with one: change both together. */
  rise() {
    if (this.element.hidden) return;
    this.element.style.top = '';
    if (!DESK.matches) return;
    const top = parseFloat(getComputedStyle(this.element).top);
    const held = token('--sticky-top');
    this.element.style.top = `${Math.max(held, top - scrollY)}px`;
  }

  /** Opens the column, and closes the other.
   * @returns {boolean} whether it was closed before */
  show() {
    const opened = !this.open;
    this.open = true;
    this.element.hidden = false;
    if (!RISES) this.rise();
    this.scrim.hidden = false;
    // The next frame, so that the column slides in from where it was drawn closed; unless it
    // closed meanwhile.
    requestAnimationFrame(() => {
      if (this.open) this.element.classList.add('open');
    });
    if (opened) document.dispatchEvent(new CustomEvent(OPENED, { detail: this }));
    return opened;
  }

  /** @param {boolean} refocus whether the focus goes back to what opened the column */
  close(refocus) {
    if (!this.open) return;
    this.open = false;
    this.element.classList.remove('open', 'full');
    // Hidden once it faded out (left-column.css), unless it opened again meanwhile.
    setTimeout(() => {
      if (!this.open) this.element.hidden = true;
    }, token('--drawer-out'));
    this.scrim.hidden = true;
    this.closed(refocus);
  }

  /**
   * The phone sheet's grabber: a drag up opens the sheet to the full height, a drag down brings it
   * back to two thirds, or closes it.
   * @param {HTMLElement} grabber
   */
  dragSheet(grabber) {
    /** @type {number | null} */
    let from = null;
    grabber.addEventListener('pointerdown', (event) => {
      from = event.clientY;
      grabber.setPointerCapture(event.pointerId);
    });
    grabber.addEventListener('pointerup', (event) => {
      if (from === null) return;
      const moved = event.clientY - from;
      from = null;
      const full = this.element.classList.contains('full');
      if (moved < -DRAG) this.element.classList.add('full');
      else if (moved > DRAG && full) this.element.classList.remove('full');
      else if (moved > DRAG) this.close(false);
    });
  }
}
