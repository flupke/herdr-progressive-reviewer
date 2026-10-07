// The masthead, one row above every screen (docs/design/explore-page/README.md, "Masthead"):
// the chat's bubble at its left on a desktop, the product's name, the review's revision and
// title, the round rail, and the ⋯ menu, with the bubble beside it below the desk's two
// columns. "Design ▾" on the rail opens the design map in place; the menu copies the round's
// link, opens the agent's conversation (chat.js), and holds Reset, behind its confirmation,
// which the page offers nowhere else. The masthead's bottom hairline is an element of its
// own, which the meter draws on. On a phone the rail shows chips: the design, the current
// step, the screen in view and the screens beside it, and the review's title moves into the menu
// (masthead.css, swipe.css). The browser tab's title says whose turn it is, after the count of
// the agent's replies the reviewer has not seen.
//
// The rail and the tab title come from the round's overview, which the tool derives: every
// question number here is a step of the rail, never a count of the question's versions. While
// the quiz shows an item, the page hands over a rail whose quiz step names it (railShowing in
// quiz.js). Each done question step opens its earlier question, and the done quiz step the
// answered quiz (route.js); while the page shows the design, an earlier question or the answered
// quiz, the rail shows that step as the one in view and the round's own step as the next one, a
// link back to it. On a phone a swipe toward a screen fills its chip
// (swipe.js).

/** @import { DesignView, PageView, RailStep, ReviewName, Step, TabTitle } from "./types.ts" */
/** @import { Notifier, NotifyState } from "./notify.js" */

import { requestChat } from './chat.js';
import { DESK } from './desk.js';
import { h, keyOf, markdown, Region } from './dom.js';
import { revisionCode, revisionText } from './revision.js';
import { earlierQuestion, quizItem, STAGE } from './route.js';

/** What the masthead has open: the design map, the menu, or nothing. */
/** @typedef {'map' | 'menu' | null} Open */
/** The screen the page shows other than the round's stage: the design, an earlier question, or
 * the answered quiz.
 * @typedef {{ kind: 'design' } | { kind: 'question', number: number } | { kind: 'quiz' } | null} Viewing */

const RESET_HINT =
  'Reset closes this round for good: it is no longer shown, its agent can no longer post to it, and the page offers to start a new round. Its records stay saved.';

export class Masthead {
  /**
   * @param {HTMLElement} header the page's `header.masthead`
   * @param {() => void} redraw draws the page again from its latest view, after the masthead's
   *   own state changed
   * @param {Notifier} notifier the notifications the menu turns on and off
   */
  constructor(header, redraw, notifier) {
    this.header = header;
    this.redraw = redraw;
    this.notifier = notifier;
    // The product's name and the review's title, in the row's free space.
    const identity = h('div', { class: 'identity' }, h('p', { class: 'product' }, 'Explore'));
    header.replaceChildren(identity);
    this.review = new Region(identity, 'review');
    this.rail = new Region(header, 'rail');
    this.menu = new Region(header, 'menu');
    // The chat's bubble goes here: first in the row on a desktop, beside the menu below.
    this.chatPlace = h('div', { class: 'masthead-chat', id: 'masthead-chat' });
    this.placeChat();
    DESK.addEventListener('change', () => this.placeChat());
    // The hairline under the masthead, across the window: the meter draws on it.
    header.append(h('div', { class: 'masthead-line', id: 'masthead-line' }));
    /** @type {Open} */
    this.open = null;
    /** Whether the menu shows Reset's confirmation in place of its entries. */
    this.confirming = false;
    /** The round Reset closes, as the latest view offers it. @type {string | null} */
    this.resets = null;
    /** The steps of the screens beside the one the page shows, whose chips a phone shows.
     * @type {Set<string | undefined>} */
    this.near = new Set();
    /** The step of the screen a swipe would turn to, whose chip fills. @type {string | null} */
    this.aimed = null;
    document.addEventListener('click', (event) => this.clickOutside(event));
    addEventListener('resize', () => this.fit());
    document.addEventListener('keydown', (event) => {
      if (event.key !== 'Escape' || !this.open) return;
      // Escape closes the popover, and nothing else: the chat stays open (chat.js).
      event.preventDefault();
      this.close(true);
    });
  }

  /**
   * @param {PageView} view
   * @param {Viewing} viewing the screen the page shows, other than the stage, which the rail then
   *   shows as the step in view, the round's own step next
   * @param {number} [unread] how many of the agent's replies the tab's title counts as unread
   * @param {boolean} [quizLink] whether the rail's quiz step opens the answered quiz
   */
  update(view, viewing, unread = 0, quizLink = false) {
    const cover = view.start !== null;
    this.review.show(keyOf([view.review, cover]), () => reviewLine(view.review, cover));
    const rail = view.rail;
    // A new step on the rail rebuilds it: "Design ▾" keeps the focus it had.
    const focused = document.activeElement === this.toggleOf('map');
    this.rail.show(keyOf([rail, view.design, viewing, quizLink]), () =>
      rail.length > 0
        ? railNav(rail, view.design, viewing, quizLink, () => this.toggle('map'), () => this.close(false))
        : null,
    );
    if (focused && document.activeElement !== this.toggleOf('map')) this.toggleOf('map')?.focus();
    this.markChips();
    if (view.reset !== this.resets) {
      this.resets = view.reset;
      this.confirming = false;
    }
    this.drawMenu(view);
    this.showOpen();
    const title = tabTitle(view.title, view.review);
    document.title = unread > 0 ? `(${unread}) ${title}` : title;
  }

  /** @param {PageView} view */
  drawMenu(view) {
    const round = view.reset;
    const chat = view.conversation !== null;
    const notify = this.notifier.state();
    this.menu.show(keyOf([view.review, round, view.rail.length > 0, this.confirming, chat, notify]), () =>
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
                chat
                  ? h(
                      'button',
                      {
                        class: 'menu-item',
                        type: 'button',
                        onclick: () => {
                          this.close(false);
                          requestChat();
                        },
                      },
                      "Open the agent's conversation",
                    )
                  : null,
                notifyItem(notify, () => this.toggleNotify()),
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

  /**
   * Marks the step of the screen a swipe would turn to, or unmarks it: its chip fills.
   * @param {string} step the step's screen: `design`, `question-N` or `round`
   * @param {boolean} on
   */
  target(step, on) {
    if (on) this.aimed = step;
    else if (this.aimed === step) this.aimed = null;
    this.markChips();
  }

  /**
   * Names the steps of the screens beside the one the page shows, which a swipe turns to: a
   * phone shows their chips.
   * @param {(string | undefined)[]} steps
   */
  neighbours(steps) {
    this.near = new Set(steps);
    this.markChips();
  }

  /** Marks the chips of the neighbours and of the swipe's target, again after any redraw. */
  markChips() {
    for (const step of this.header.querySelectorAll('.rail .step[data-screen]')) {
      if (!(step instanceof HTMLElement)) continue;
      step.classList.toggle('near', this.near.has(step.dataset.screen));
      step.classList.toggle('target', step.dataset.screen === this.aimed);
    }
    this.fit();
  }

  /** On a narrow window, drops the chips of the neighbours when the row has no room for them
   * beside the product's name (class `crowded`): the design's chip and the current step stay. */
  fit() {
    const identity = this.header.querySelector('.identity');
    const product = this.header.querySelector('.product');
    if (!(identity instanceof HTMLElement) || !(product instanceof HTMLElement)) return;
    this.header.classList.remove('crowded');
    this.header.classList.toggle('crowded', identity.getBoundingClientRect().width < product.scrollWidth);
  }

  /** Puts the chat's place where the window's width draws it, so that the keyboard reaches the
   * bubble in the order the row shows it; the bubble keeps the focus it had. */
  placeChat() {
    const focused = document.activeElement;
    if (DESK.matches) this.header.prepend(this.chatPlace);
    else this.rail.anchor.after(this.chatPlace);
    if (focused instanceof HTMLElement && this.chatPlace.contains(focused)) focused.focus();
  }

  /** Turns the agent's notifications on or off. The menu is drawn again for its new state, hidden
   * until it opens again, so the entry gets its focus back once it shows. */
  async toggleNotify() {
    await this.notifier.toggle();
    const entry = this.header.querySelector('#round-menu [data-focus-key="notify"]');
    if (entry instanceof HTMLElement) entry.focus({ preventScroll: true });
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
    // The menu opens drawn afresh: the browser's notification permission may have changed since.
    if (what === 'menu') this.redraw();
    else this.showOpen();
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
 * The review's revision, then its title, as the pane's header orders them; on the start cover,
 * whose headline is the title, the repository.
 * @param {ReviewName | null} review
 * @param {boolean} cover
 */
function reviewLine(review, cover) {
  if (!review) return null;
  return h('p', { class: 'review' }, cover ? review.repository : reviewParts(review));
}

/** @param {ReviewName} review */
function reviewParts(review) {
  return [revisionCode(review.revision), review.title ? [' ', h('span', { class: 'review-title' }, review.title)] : null];
}

/**
 * The round rail: done steps with a check, the current step with its state, later steps muted.
 * Each done question opens its earlier question, and the done quiz the answered quiz.
 * @param {RailStep[]} rail
 * @param {DesignView | null} design
 * @param {Viewing} viewing the screen the page shows other than the stage: its step then shows
 *   as current, and the round's own step as the next one, which leads back to the stage
 * @param {boolean} quizLink whether the quiz step opens the answered quiz
 * @param {() => void} toggleMap
 * @param {() => void} closeMap
 */
function railNav(rail, design, viewing, quizLink, toggleMap, closeMap) {
  return h(
    'nav',
    { class: 'rail', 'aria-label': 'Round' },
    h(
      'ol',
      {},
      rail.map((step) => {
        const isDesign = step.step.kind === 'design';
        const viewed = isViewed(step.step, viewing);
        const state = viewed ? 'current' : step.state.kind;
        const shown = viewing && !viewed && state === 'current' ? 'next' : state;
        // A done question keeps its check while its screen is viewed; the design viewed reads
        // as the step in view, as on the round's first screen.
        const done = step.state.kind === 'done' && !(isDesign && viewed);
        const name = [
          done ? h('span', { class: 'check', 'aria-hidden': 'true' }, '✓') : null,
          done ? ' ' : null,
          stepName(step.step),
          step.state.kind === 'current' && step.state.working ? ' · working' : null,
        ];
        const current = step.state.kind === 'current' ? 'step' : viewed ? 'page' : null;
        const link = isDesign && design ? null : stepLink(step, viewing, quizLink, name);
        // A step that is a link says it is the current one on its link.
        if (link && current) link.setAttribute('aria-current', current);
        return h(
          'li',
          {
            class: `step ${shown}${isDesign ? ' design-step' : ''}`,
            'aria-current': link ? null : current,
            'data-screen': screenOf(step, quizLink),
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
            : (link ?? name),
        );
      }),
    ),
  );
}

/**
 * A link for a step that leads to a screen: a done question to its earlier question, the quiz to
 * the first item of the answered quiz when the page has it (page.js), and the round's own step
 * back to the stage while the page shows another screen; `null` for another.
 * @param {RailStep} step
 * @param {Viewing} viewing
 * @param {boolean} quizLink whether the quiz step opens the answered quiz
 * @param {(Node | string | null)[]} name
 */
function stepLink(step, viewing, quizLink, name) {
  if (step.step.kind === 'question' && step.state.kind === 'done' && !isViewed(step.step, viewing)) {
    return h('a', { class: 'step-link', href: earlierQuestion(step.step.number) }, name);
  }
  if (step.step.kind === 'quiz' && quizLink && !isViewed(step.step, viewing)) {
    return h('a', { class: 'step-link', href: quizItem(1) }, name);
  }
  if (step.state.kind === 'current' && viewing) return h('a', { class: 'step-link', href: STAGE }, name);
  return null;
}

/** Whether `step` is the screen the page shows other than the stage.
 * @param {Step} step
 * @param {Viewing} viewing */
function isViewed(step, viewing) {
  if (!viewing) return false;
  if (viewing.kind === 'design') return step.kind === 'design';
  if (viewing.kind === 'quiz') return step.kind === 'quiz';
  return step.kind === 'question' && step.number === viewing.number;
}

/** The screen a step leads to, which a swipe names to fill the step's chip: `design`, an earlier
 * question's `question-N`, `quiz` for the answered quiz, `round` for the round's own step, or
 * `null`.
 * @param {RailStep} step
 * @param {boolean} quizLink whether the quiz step opens the answered quiz */
function screenOf(step, quizLink) {
  if (step.state.kind === 'current') return 'round';
  if (step.step.kind === 'design') return 'design';
  if (step.step.kind === 'question' && step.state.kind === 'done') return `question-${step.step.number}`;
  if (step.step.kind === 'quiz' && quizLink) return 'quiz';
  return null;
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
 * The menu entry that turns the agent's notifications on and off, or says why this browser
 * cannot show them.
 * @param {NotifyState} state
 * @param {() => void} toggle
 */
function notifyItem(state, toggle) {
  // An entry the browser does not allow stays in the tab order, so that its note is heard.
  const unavailable = state === 'unavailable' || state === 'blocked';
  return h(
    'button',
    {
      class: 'menu-item',
      type: 'button',
      'aria-pressed': state === 'on' ? 'true' : 'false',
      'aria-disabled': unavailable ? 'true' : false,
      'data-focus-key': 'notify',
      onclick: toggle,
    },
    'Notify me when the agent finishes',
    h('span', { class: 'menu-note' }, NOTIFY_NOTES[state]),
  );
}

/** What the notifications' entry says of their state. @type {Record<NotifyState, string>} */
const NOTIFY_NOTES = {
  on: 'on',
  off: 'off',
  blocked: "blocked in the browser's settings",
  unavailable: 'needs localhost or the tunnel',
};

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
  const name = review ? review.title || revisionText(review.revision) : null;
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
