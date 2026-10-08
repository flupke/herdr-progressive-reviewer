// The swipe between screens, on a phone (docs/design/explore-page/README.md, "Swipe between
// screens"; prototype SwipeDemo.dc.html): the design, each earlier question and the round's
// current stage sit side by side in the rail's order. A sideways drag moves the screen with the
// finger, its neighbour beside it; past the threshold (48 pixels), the neighbour's chip on the
// rail fills, a 3-pixel accent edge lights on the side the reviewer heads to, and the phone ticks
// where it can. Released past the threshold, or flicked faster than 0.5 pixels a millisecond,
// the page turns to the neighbour in 280 milliseconds; released before, it springs back. At either
// end, the screen follows the finger at 0.35 of its pace. A frame that scrolls sideways (a
// table, a diagram) keeps the gesture for its own scroll.
//
// The turn changes the address (route.js), so the browser's Back returns to the screen before.

/**
 * One screen of the track: the address that shows it, its element, and the step of the rail
 * that names it (masthead.js, `data-screen`).
 * @typedef {{ address: string, element: HTMLElement, step: string }} Screen
 * @typedef {{ screens: Screen[], shown: number }} Track
 */

/** How far a drag goes before it turns the page, in pixels. */
const THRESHOLD = 48;
/** How fast a flick goes to turn the page, in pixels per millisecond. */
const FLICK = 0.5;
/** How far a pointer moves before the gesture is a swipe or a scroll. */
const SLOP = 8;
/** How much of the drag the screen follows at either end of the track. */
const RUBBER_BAND = 0.35;
/** How long the turn and the spring back take, in milliseconds. */
const SNAP = 280;
const EASING = 'cubic-bezier(.2, .8, .2, 1)';
/** The page's phone layout, the only one that swipes. */
const PHONE = matchMedia('(max-width: 40rem)');

export class Swipe {
  /**
   * @param {HTMLElement} main the page's `main`, which holds the screens
   * @param {{ track: () => Track, settled: (address: string | null, top: number) => void,
   *   chip: (step: string, on: boolean) => void }} page what the page tells and does: its track,
   *   the end of a swipe, which turned to the screen at `address`, whose top it left at `top` in
   *   the window, or sprang back (`null`), and the fill of a step's chip
   */
  constructor(main, page) {
    this.main = main;
    this.page = page;
    /** The gesture under way, from its first pointer down. @type {Gesture | null} */
    this.gesture = null;
    /** Whether a turn or a spring back is playing. */
    this.settling = false;
    /** Whether the click that ends a drag is to be swallowed. */
    this.swallowClick = false;
    this.edges = {
      left: edge('left'),
      right: edge('right'),
    };
    document.body.append(this.edges.left, this.edges.right);
    main.addEventListener('pointerdown', (event) => this.down(event));
    main.addEventListener('pointermove', (event) => this.move(event));
    main.addEventListener('pointerup', (event) => this.up(event));
    main.addEventListener('pointercancel', () => this.cancel());
    // A drag that turned or sprang back is not a click on what it started on.
    main.addEventListener(
      'click',
      (event) => {
        if (!this.swallowClick) return;
        this.swallowClick = false;
        event.preventDefault();
        event.stopPropagation();
      },
      true,
    );
  }

  /** Whether a swipe moves the screens: the page leaves them as they are until it ends. */
  get active() {
    return this.settling || Boolean(this.gesture?.drag);
  }

  /** @param {PointerEvent} event */
  down(event) {
    this.swallowClick = false;
    if (this.gesture || this.settling || !event.isPrimary || event.button !== 0 || !PHONE.matches) return;
    if (!(event.target instanceof Element) || keepsGesture(event.target, this.main)) return;
    this.gesture = {
      pointer: event.pointerId,
      x0: event.clientX,
      y0: event.clientY,
      lastX: event.clientX,
      lastT: event.timeStamp,
      velocity: 0,
      dx: 0,
      drag: null,
    };
  }

  /** @param {PointerEvent} event */
  move(event) {
    const gesture = this.gesture;
    if (!gesture || event.pointerId !== gesture.pointer) return;
    const dx = event.clientX - gesture.x0;
    const dy = event.clientY - gesture.y0;
    if (!gesture.drag) {
      if (Math.abs(dx) < SLOP && Math.abs(dy) < SLOP) return;
      // A vertical gesture is the page's scroll.
      if (Math.abs(dy) >= Math.abs(dx)) {
        this.gesture = null;
        return;
      }
      gesture.drag = this.startDrag();
      if (!gesture.drag) {
        this.gesture = null;
        return;
      }
      this.main.setPointerCapture?.(event.pointerId);
      getSelection()?.removeAllRanges();
    }
    const elapsed = Math.max(1, event.timeStamp - gesture.lastT);
    gesture.velocity = (event.clientX - gesture.lastX) / elapsed;
    gesture.lastX = event.clientX;
    gesture.lastT = event.timeStamp;
    gesture.dx = dx;
    this.follow(gesture.drag, dx);
  }

  /** @param {PointerEvent} event */
  up(event) {
    const gesture = this.gesture;
    if (!gesture || event.pointerId !== gesture.pointer) return;
    this.gesture = null;
    const drag = gesture.drag;
    if (!drag) return;
    this.swallowClick = true;
    // A touch drag fires no click: the next activation is not this drag's.
    setTimeout(() => {
      this.swallowClick = false;
    });
    const { dx } = gesture;
    // A finger that held still before it let go flicks nothing.
    const velocity = event.timeStamp - gesture.lastT > 100 ? 0 : gesture.velocity;
    const forward = (dx <= -THRESHOLD || velocity < -FLICK) && drag.next !== null;
    const backward = (dx >= THRESHOLD || velocity > FLICK) && drag.previous !== null;
    const to = forward ? drag.next : backward ? drag.previous : null;
    this.settle(drag, to, forward ? -1 : backward ? 1 : 0);
  }

  cancel() {
    const drag = this.gesture?.drag;
    this.gesture = null;
    if (drag) this.settle(drag, null, 0);
  }

  /** The drag of the screen that shows, with its neighbours beside it, or `null` when the page
   * shows a single screen. @returns {Drag | null} */
  startDrag() {
    const { screens, shown } = this.page.track();
    const current = screens[shown];
    if (!current || screens.length < 2) return null;
    const box = current.element.getBoundingClientRect();
    // The neighbours show from their top, where the screen's top is, or at the window's top once
    // the reviewer scrolled past it.
    const top = Math.max(box.top, 0);
    const drag = {
      current,
      previous: screens[shown - 1] ?? null,
      next: screens[shown + 1] ?? null,
      width: box.width + 2 * box.left,
      top,
      box,
      armed: /** @type {Screen | null} */ (null),
    };
    this.main.classList.add('swiping');
    for (const [neighbour, side] of /** @type {const} */ ([
      [drag.previous, -1],
      [drag.next, 1],
    ])) {
      if (!neighbour) continue;
      const element = neighbour.element;
      element.hidden = false;
      element.classList.add('swipe-neighbour');
      Object.assign(element.style, {
        top: `${top}px`,
        left: `${box.left}px`,
        width: `${box.width}px`,
        transform: `translateX(${side * drag.width}px)`,
      });
    }
    return drag;
  }

  /**
   * Moves the screens with the finger, `dx` pixels from where the drag started, and arms the
   * turn past the threshold.
   * @param {Drag} drag
   * @param {number} dx
   */
  follow(drag, dx) {
    const toward = dx < 0 ? drag.next : drag.previous;
    const offset = toward ? dx : dx * RUBBER_BAND;
    this.place(drag, offset, false);
    const armed = toward && Math.abs(dx) >= THRESHOLD ? toward : null;
    if (armed === drag.armed) return;
    if (drag.armed) this.page.chip(drag.armed.step, false);
    drag.armed = armed;
    if (armed) {
      this.page.chip(armed.step, true);
      navigator.vibrate?.(10);
    }
    this.edges.left.classList.toggle('lit', armed !== null && dx > 0);
    this.edges.right.classList.toggle('lit', armed !== null && dx < 0);
    for (const element of [this.edges.left, this.edges.right]) element.style.top = `${drag.top}px`;
  }

  /**
   * Puts the screen `offset` pixels from its place, its neighbours beside it.
   * @param {Drag} drag
   * @param {number} offset
   * @param {boolean} animate
   */
  place(drag, offset, animate) {
    const transition = animate ? `transform ${SNAP}ms ${EASING}` : 'none';
    drag.current.element.style.transition = transition;
    drag.current.element.style.transform = `translateX(${offset}px)`;
    if (drag.previous) {
      drag.previous.element.style.transition = transition;
      drag.previous.element.style.transform = `translateX(${offset - drag.width}px)`;
    }
    if (drag.next) {
      drag.next.element.style.transition = transition;
      drag.next.element.style.transform = `translateX(${offset + drag.width}px)`;
    }
  }

  /**
   * Turns to `to`, sliding the screens by one width in `direction` (-1 to the next, 1 to the
   * previous), or springs back when `to` is `null`; then leaves the screens as the address says.
   * @param {Drag} drag
   * @param {Screen | null} to
   * @param {number} direction
   */
  settle(drag, to, direction) {
    this.settling = true;
    this.place(drag, to ? direction * drag.width : 0, true);
    const done = () => {
      this.settling = false;
      if (drag.armed) this.page.chip(drag.armed.step, false);
      this.edges.left.classList.remove('lit');
      this.edges.right.classList.remove('lit');
      for (const screen of [drag.current, drag.previous, drag.next]) {
        if (!screen) continue;
        screen.element.classList.remove('swipe-neighbour');
        for (const property of ['top', 'left', 'width', 'transform', 'transition']) {
          screen.element.style.removeProperty(property);
        }
      }
      this.main.classList.remove('swiping');
      if (!to) for (const screen of [drag.previous, drag.next]) if (screen) screen.element.hidden = true;
      // The page shows the screen it turns to before the browser paints again.
      this.page.settled(to ? to.address : null, drag.top);
    };
    if (matchMedia('(prefers-reduced-motion: reduce)').matches) done();
    else setTimeout(done, SNAP);
  }
}

/**
 * A gesture from its pointer down: where it started, the latest move and its velocity, and the
 * drag once it is a sideways one.
 * @typedef {{ pointer: number, x0: number, y0: number, lastX: number, lastT: number,
 *   velocity: number, dx: number, drag: Drag | null }} Gesture
 * @typedef {{ current: Screen, previous: Screen | null, next: Screen | null, width: number,
 *   top: number, box: DOMRect, armed: Screen | null }} Drag
 */

/** The edge that lights on one side of the screen. @param {'left' | 'right'} side */
function edge(side) {
  const element = document.createElement('div');
  element.className = `swipe-edge ${side}`;
  element.setAttribute('aria-hidden', 'true');
  return element;
}

/**
 * Whether a gesture that starts on `target` belongs to it rather than to the swipe: a text box,
 * or a frame that scrolls sideways (a table, a diagram).
 * @param {Element} target
 * @param {Element} main
 */
function keepsGesture(target, main) {
  if (target.closest('input, textarea, select, [contenteditable]')) return true;
  for (let element = /** @type {Element | null} */ (target); element && element !== main; element = element.parentElement) {
    if (element.scrollWidth <= element.clientWidth + 1) continue;
    const overflow = getComputedStyle(element).overflowX;
    if (overflow === 'auto' || overflow === 'scroll') return true;
  }
  return false;
}
