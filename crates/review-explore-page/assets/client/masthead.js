// The masthead, one row above every screen (docs/design/explore-page/README.md, "Masthead"):
// the product's name, the review's title, the round rail, a place for the chat's bubble, and the
// ⋯ menu. "Design ▾" on the rail opens the design map in place; the menu copies the round's
// link and holds Reset, behind its confirmation, which the page offers nowhere else. The
// masthead's bottom hairline is an element of its own, which the meter draws on. On a phone the
// rail shows two chips, the design and the current step, and the review's title moves into the
// menu (masthead.css). The browser tab's title says whose turn it is.
//
// The rail and the tab title come from the round's overview, which the tool derives: every
// question number here is a step of the rail, never a count of the question's versions. While
// the quiz shows an item, the page hands over a rail whose quiz step names it (railShowing in
// quiz.js).

/** @import { DesignView, PageView, RailStep, ReviewName, Step, TabTitle } from "./types.ts" */

import { h, keyOf, markdown, Region } from './dom.js';

/** What the masthead has open: the design map, the menu, or nothing. */
/** @typedef {'map' | 'menu' | null} Open */

const RESET_HINT =
  'Reset closes this round for good: it is no longer shown, its agent can no longer post to it, and the page offers to start a new round. Its records stay saved.';

export class Masthead {
  /**
   * @param {HTMLElement} header the page's `header.masthead`
   * @param {() => void} redraw draws the page again from its latest view, after the masthead's
   *   own state changed
   */
  constructor(header, redraw) {
    this.header = header;
    this.redraw = redraw;
    // The product's name and the review's title, in the row's free space.
    const identity = h('div', { class: 'identity' }, h('p', { class: 'product' }, 'Explore'));
    header.replaceChildren(identity);
    this.review = new Region(identity, 'review');
    this.rail = new Region(header, 'rail');
    // The chat's bubble goes here, beside the menu.
    header.append(h('div', { class: 'masthead-chat', id: 'masthead-chat' }));
    this.menu = new Region(header, 'menu');
    // The hairline under the masthead, across the window: the meter draws on it.
    header.append(h('div', { class: 'masthead-line', id: 'masthead-line' }));
    /** @type {Open} */
    this.open = null;
    /** Whether the menu shows Reset's confirmation in place of its entries. */
    this.confirming = false;
    /** The round Reset closes, as the latest view offers it. @type {string | null} */
    this.resets = null;
    document.addEventListener('click', (event) => this.clickOutside(event));
    document.addEventListener('keydown', (event) => {
      if (event.key === 'Escape' && this.open) this.close(true);
    });
  }

  /**
   * @param {PageView} view
   * @param {boolean} viewingDesign whether the page shows the design screen, which the rail then
   *   shows as the step in view, the round's own step next
   */
  update(view, viewingDesign) {
    const cover = view.start !== null;
    this.review.show(keyOf([view.review, cover]), () => reviewLine(view.review, cover));
    const rail = view.rail;
    // A new step on the rail rebuilds it: "Design ▾" keeps the focus it had.
    const focused = document.activeElement === this.toggleOf('map');
    this.rail.show(keyOf([rail, view.design, viewingDesign]), () =>
      rail.length > 0
        ? railNav(rail, view.design, viewingDesign, () => this.toggle('map'), () => this.close(false))
        : null,
    );
    if (focused && document.activeElement !== this.toggleOf('map')) this.toggleOf('map')?.focus();
    if (view.reset !== this.resets) {
      this.resets = view.reset;
      this.confirming = false;
    }
    this.drawMenu(view);
    this.showOpen();
    document.title = tabTitle(view.title, view.review);
  }

  /** @param {PageView} view */
  drawMenu(view) {
    const round = view.reset;
    this.menu.show(keyOf([view.review, round, view.rail.length > 0, this.confirming]), () =>
      h(
        'div',
        { class: 'menu' },
        h(
          'button',
          {
            class: 'menu-toggle',
            type: 'button',
            'aria-label': 'Round menu',
            'aria-expanded': 'false',
            'aria-controls': 'round-menu',
            onclick: () => this.toggle('menu'),
          },
          '⋯',
        ),
        h(
          'div',
          { class: 'menu-popover', id: 'round-menu', hidden: true },
          view.review ? h('p', { class: 'menu-review' }, reviewParts(view.review)) : null,
          round !== null && this.confirming
            ? resetConfirmation(round)
            : [
                copyLink(view.rail.length > 0 ? "Copy the round's link" : "Copy the page's link"),
                round !== null
                  ? [
                      h('hr', {}),
                      h(
                        'button',
                        { class: 'menu-item', type: 'button', onclick: () => this.confirmReset() },
                        'Reset this round…',
                        h('span', { class: 'menu-note' }, 'closes it for good'),
                      ),
                    ]
                  : null,
              ],
        ),
      ),
    );
  }

  /** Replaces the menu's entries with Reset's hint and its Confirm reset. */
  confirmReset() {
    this.confirming = true;
    this.redraw();
    const confirm = this.header.querySelector('#round-menu .button.danger');
    if (confirm instanceof HTMLElement) confirm.focus();
  }

  /** @param {Open} what */
  toggle(what) {
    if (this.open === what) {
      this.close(false);
      return;
    }
    this.open = what;
    this.showOpen();
  }

  /** @param {boolean} refocus whether to give the focus back to the button that opened it */
  close(refocus) {
    const was = this.open;
    this.open = null;
    if (this.confirming) {
      this.confirming = false;
      this.redraw();
    }
    this.showOpen();
    if (refocus && was) this.toggleOf(was)?.focus();
  }

  /** @param {'map' | 'menu'} what */
  toggleOf(what) {
    const button = this.header.querySelector(what === 'map' ? '.design-toggle' : '.menu-toggle');
    return button instanceof HTMLElement ? button : null;
  }

  /** Shows what is open, and hides the rest, after any redraw. */
  showOpen() {
    for (const [what, popover] of /** @type {const} */ ([
      ['map', '#design-map'],
      ['menu', '#round-menu'],
    ])) {
      const open = this.open === what;
      this.toggleOf(what)?.setAttribute('aria-expanded', String(open));
      const element = this.header.querySelector(popover);
      if (element instanceof HTMLElement) element.hidden = !open;
    }
  }

  /** Closes what is open when the reviewer clicks elsewhere.
   * @param {MouseEvent} event */
  clickOutside(event) {
    if (!this.open) return;
    const container = this.open === 'map' ? 'design-step' : 'menu';
    // The path, rather than the target's ancestors: a click that redraws the menu detaches its
    // target from the page.
    const inside = event.composedPath().some((node) => node instanceof Element && node.classList.contains(container));
    if (!inside) this.close(false);
  }
}

/**
 * The review's title with its revision; on the start cover, whose headline is the title, the
 * repository.
 * @param {ReviewName | null} review
 * @param {boolean} cover
 */
function reviewLine(review, cover) {
  if (!review) return null;
  return h('p', { class: 'review' }, cover ? review.repository : reviewParts(review));
}

/** @param {ReviewName} review */
function reviewParts(review) {
  return [review.title ? [h('span', { class: 'review-title' }, review.title), ' '] : null, h('code', {}, review.revision)];
}

/**
 * The round rail: done steps with a check, the current step with its state, later steps muted.
 * @param {RailStep[]} rail
 * @param {DesignView | null} design
 * @param {boolean} viewingDesign whether the page shows the design screen: the design step then
 *   shows as current, and the round's own step as the next one
 * @param {() => void} toggleMap
 * @param {() => void} closeMap
 */
function railNav(rail, design, viewingDesign, toggleMap, closeMap) {
  return h(
    'nav',
    { class: 'rail', 'aria-label': 'Round' },
    h(
      'ol',
      {},
      rail.map((step) => {
        const isDesign = step.step.kind === 'design';
        const state = viewingDesign && isDesign ? 'current' : step.state.kind;
        const shown = viewingDesign && !isDesign && state === 'current' ? 'next' : state;
        const done = state === 'done';
        const name = [
          done ? h('span', { class: 'check', 'aria-hidden': 'true' }, '✓') : null,
          done ? ' ' : null,
          stepName(step.step),
          step.state.kind === 'current' && step.state.working ? ' · working' : null,
        ];
        return h(
          'li',
          {
            class: `step ${shown}${isDesign ? ' design-step' : ''}`,
            'aria-current': step.state.kind === 'current' ? 'step' : viewingDesign && isDesign ? 'page' : null,
          },
          isDesign && design
            ? [
                h(
                  'button',
                  {
                    class: 'design-toggle',
                    type: 'button',
                    'aria-expanded': 'false',
                    'aria-controls': 'design-map',
                    onclick: toggleMap,
                  },
                  name,
                  h('span', { class: 'caret', 'aria-hidden': 'true' }, '▾'),
                ),
                designMap(design, closeMap),
              ]
            : name,
        );
      }),
    ),
  );
}

/** @param {Step} step */
function stepName(step) {
  switch (step.kind) {
    case 'design':
      return 'Design';
    case 'question':
      return `Q${step.number}`;
    case 'quiz': {
      const stage = step.stage;
      if (stage.kind === 'running') return `Quiz ${stage.item}/${stage.items}`;
      if (stage.kind === 'scored') return `Quiz ${stage.correct}/${stage.items}`;
      return 'Quiz';
    }
    case 'conclusion':
      return 'Conclusion';
  }
}

/**
 * The design map: the change's thesis, its four parts with their gist, each a link to its part
 * of the design screen, and a link to the whole design screen (route.js).
 * @param {DesignView} design
 * @param {() => void} close
 */
function designMap(design, close) {
  const follow = () => close();
  return h(
    'div',
    { class: 'design-map', id: 'design-map', role: 'group', 'aria-labelledby': 'design-map-title', hidden: true },
    h('p', { class: 'eyebrow', id: 'design-map-title' }, 'Design of the change'),
    markdown(design.thesis_html, 'map-thesis'),
    h(
      'ol',
      { class: 'map-parts' },
      design.parts.map((part, index) =>
        h(
          'li',
          {},
          h(
            'a',
            { href: `#design-part-${index + 1}`, onclick: follow },
            h('span', { class: 'disc', 'aria-hidden': 'true' }, String(index + 1)),
            h('span', { class: 'part-name' }, part.title),
            markdown(part.thesis_html, 'part-gist'),
          ),
        ),
      ),
    ),
    h('a', { class: 'map-open', href: '#design', onclick: follow }, 'Open the design →'),
  );
}

/**
 * The menu entry that copies the page's address, which opens the round in another browser.
 * @param {string} label
 */
function copyLink(label) {
  const button = h('button', { class: 'menu-item', type: 'button' }, label);
  const told = h('span', { class: 'sr-only', 'aria-live': 'polite' });
  button.addEventListener('click', async () => {
    const copied = await copy(location.href);
    // The older copy command takes the focus away to a text box of its own.
    button.focus();
    if (!copied) {
      // The browser refuses the clipboard: the address shows, selected, for the reviewer to copy.
      const field = h('input', { class: 'menu-link', type: 'text', readonly: true, value: location.href, 'aria-label': 'Link' });
      button.replaceWith(field);
      field.select();
      return;
    }
    button.textContent = 'Link copied';
    told.textContent = 'Link copied';
    setTimeout(() => {
      button.textContent = label;
      told.textContent = '';
    }, 2000);
  });
  return [button, told];
}

/**
 * Copies `text` to the clipboard: with the Clipboard API where the page is a secure context
 * (its loopback address), else with the older copy command (its address on the network).
 * @param {string} text
 */
async function copy(text) {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    const field = h('textarea', { class: 'sr-only', readonly: true });
    field.value = text;
    document.body.append(field);
    field.select();
    let copied = false;
    try {
      copied = document.execCommand('copy');
    } catch {
      copied = false;
    }
    field.remove();
    return copied;
  }
}

/**
 * Reset's hint and its Confirm reset: in the menu, and, worded as starting a new round, behind
 * "Start a new round…" in the panel of a round whose implementation request the agent received
 * (conclusion.js). Both send the same request.
 * @param {string} round the round Reset closes
 * @param {{ hint: string, label: string }} [words] the hint and the button's words
 */
export function resetConfirmation(round, { hint, label } = { hint: RESET_HINT, label: 'Confirm reset' }) {
  return h(
    'form',
    { class: 'reset', 'data-method': 'reset' },
    h('input', { type: 'hidden', name: 'round', value: round }),
    h('p', { class: 'hint' }, hint),
    h('button', { class: 'button danger', type: 'submit' }, label),
  );
}

/**
 * The browser tab's title: whose turn it is, then the review.
 * @param {TabTitle | null} title
 * @param {ReviewName | null} review
 */
function tabTitle(title, review) {
  const name = review ? review.title || review.revision : null;
  if (!title) return name ? `Explore — ${name}` : 'Explore';
  return `${turn(title)} — ${name ?? 'Explore'}`;
}

/** @param {TabTitle} title */
function turn(title) {
  switch (title.kind) {
    case 'your_turn':
      return `Q${title.question} · your turn`;
    case 'agent_working':
      return 'Agent working…';
    case 'retry_needed':
      return 'Retry needed';
    case 'conclusion':
      return 'Conclusion';
  }
}
