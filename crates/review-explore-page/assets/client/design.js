// The design screen: the design of the change as a screen of its own, which opens the round and
// which the reviewer opens again from any later stage (route.js has its addresses). The reading
// column holds the change's thesis and its four parts, each led by its thesis; the panel, the
// design map, marks the part in view and ends on the way back to the round. On a phone the map
// sits under the thesis, each part's header stays at the top of the screen while the part
// scrolls, and a bar pinned to the bottom of the screen holds the way back.

/** @import { DesignView, DesignPartView, QuestionView } from "./types.ts" */

import { DOORS, doorChip } from './chips.js';
import { h, keyOf, markdown, Region, setRenderedMarkdown } from './dom.js';
import { designPart, STAGE } from './route.js';

/**
 * The step the round stands at, which the way back from the design goes to.
 * @typedef {{ kind: 'question', number: number, working: boolean } | { kind: 'quiz' }
 *   | { kind: 'conclusion' }} Current
 */

export class DesignScreen {
  constructor() {
    this.element = h('section', { class: 'design-screen desk', 'aria-label': 'Design of the change' });
    this.head = new Region(this.element, 'head');
    this.map = new Region(this.element, 'map');
    this.parts = new Region(this.element, 'parts');
    this.bar = new Region(this.element, 'bar');
    this.spy = new PartSpy(this.element);
  }

  /**
   * @param {DesignView} design
   * @param {Current | null} current
   * @param {QuestionView | null} question the question the round waits for, if any
   */
  update(design, current, question) {
    const asked = current?.kind === 'question' && question !== null ? question : null;
    this.head.show(keyOf([design.thesis_html, design.parts.length, design.minutes, design.changed_files]), () =>
      head(design),
    );
    const map = this.map.show(keyOf([design.parts, current, asked?.text_html, asked?.door]), () =>
      designMap(design.parts, current, asked),
    );
    const parts = this.parts.show(keyOf(design.parts), () => [
      ...design.parts.map(partSection),
      h('span', { class: 'design-end', 'aria-hidden': 'true' }),
    ]);
    const door = asked?.door ?? null;
    this.bar.show(keyOf([current, door]), () => (current ? bar(current, door) : null));
    if (map || parts) this.spy.watch();
  }

  /** Scrolls to part `number` of the design, or to its top, and marks that part as the one in
   * view. @param {number | null} number */
  scrollTo(number) {
    const target = number === null ? null : document.getElementById(designPart(number).slice(1));
    if (target) target.scrollIntoView();
    else window.scrollTo(0, 0);
    this.spy.reach(number ?? 1);
  }

  stop() {
    this.spy.stop();
  }
}

/** The change's thesis as the headline, and how long the design is.
 * @param {DesignView} design */
function head(design) {
  const thesis = h('h2', { id: 'design-title' });
  setRenderedMarkdown(thesis, design.thesis_html);
  const minutes = design.minutes === 1 ? 'about 1 minute' : `about ${design.minutes} minutes`;
  const files = design.changed_files === 1 ? '1 file' : `${design.changed_files} files`;
  const meta = [`${design.parts.length} parts`, minutes, ...(design.changed_files > 0 ? [files] : [])];
  return h(
    'header',
    { class: 'design-head' },
    h('p', { class: 'eyebrow' }, 'Design of the change · read before question 1'),
    thesis,
    h('p', { class: 'meta' }, meta.join(' · ')),
  );
}

/** The design map: a link to each part with its thesis, then where the round stands.
 * @param {DesignPartView[]} parts
 * @param {Current | null} current
 * @param {QuestionView | null} question */
function designMap(parts, current, question) {
  return h(
    'nav',
    { class: 'design-nav panel', 'aria-label': 'Design map' },
    h('p', { class: 'eyebrow' }, 'Design map'),
    h(
      'ol',
      {},
      parts.map((part, index) => {
        // The link is named by the part's title, and described by its gist.
        const gist = h('span', { class: 'gist', id: `design-gist-${index + 1}`, 'aria-hidden': 'true' });
        setRenderedMarkdown(gist, part.thesis_html);
        return h(
          'li',
          { 'data-part': index + 1 },
          h(
            'a',
            { href: designPart(index + 1), 'aria-describedby': gist.id },
            h('span', { class: 'n', 'aria-hidden': 'true' }, index + 1),
            h('span', { class: 'name' }, part.title),
            gist,
          ),
        );
      }),
    ),
    current ? next(current, question) : null,
  );
}

/** Under the map, where the round stands: the question it waits for, and the way to it.
 * @param {Current} current
 * @param {QuestionView | null} question */
function next(current, question) {
  return h(
    'div',
    { class: 'design-next' },
    h('p', { class: 'eyebrow' }, `Then · ${stepName(current)}`, question?.door ? doorChip(question.door) : null),
    question ? markdown(question.text_html, 'next-text') : null,
    question ? h('p', { class: 'hint' }, 'Its choices and evidence open on the next screen.') : null,
    goTo(current),
  );
}

/** The bar pinned to the bottom of a phone's screen: where the round stands, with the Door of
 * the question it waits for, and the way to it.
 * @param {Current} current
 * @param {import('./types.ts').Door | null} door */
function bar(current, door) {
  const name = stepName(current);
  const state = [`Current · ${name.charAt(0).toUpperCase()}${name.slice(1)}`];
  if (current.kind === 'question' && current.working) state.push('working');
  if (door) state.push(DOORS[door].name);
  return h('div', { class: 'design-bar' }, h('p', {}, state.join(' · ')), goTo(current));
}

/** The way to the step the round stands at, from the design or from an earlier question.
 * @param {Current} current */
export function goTo(current) {
  return h(
    'a',
    { class: 'button primary block', href: STAGE },
    `Go to ${stepName(current)}`,
    h('span', { 'aria-hidden': 'true' }, '→'),
  );
}

/** "question 2", "the quiz", "the conclusion". @param {Current} current */
function stepName(current) {
  switch (current.kind) {
    case 'question':
      return `question ${current.number}`;
    case 'quiz':
      return 'the quiz';
    case 'conclusion':
      return 'the conclusion';
  }
}

/** One part: its number, its name and "2 / 4", its thesis, then its text.
 * @param {DesignPartView} part
 * @param {number} index
 * @param {DesignPartView[]} parts */
function partSection(part, index, parts) {
  const number = index + 1;
  const id = designPart(number).slice(1);
  const thesis = h('p', { class: 'thesis' });
  setRenderedMarkdown(thesis, part.thesis_html);
  return h(
    'section',
    {
      class: 'design-part',
      id,
      'data-part': number,
      'aria-labelledby': `${id}-title`,
    },
    h(
      'header',
      { class: 'part-head' },
      h('span', { class: 'n', 'aria-hidden': 'true' }, number),
      h('h3', { class: 'eyebrow', id: `${id}-title` }, part.title),
      h('span', { class: 'part-of' }, `${number} / ${parts.length}`),
    ),
    thesis,
    markdown(part.body_html),
  );
}

/**
 * Marks the part in view, in the map and in its header, and the parts above it as read: the
 * part in view is the last one whose top passed the upper third of the window (at most 240
 * pixels down), or the last part
 * once the page is scrolled to its end. The observer only says when to look again; the links of
 * the map work without it.
 */
class PartSpy {
  /** @param {HTMLElement} screen */
  constructor(screen) {
    this.screen = screen;
    /** @type {IntersectionObserver | null} */
    this.parts = null;
    // The end of the design coming into view.
    this.ending = new IntersectionObserver(() => this.mark());
    this.onResize = () => this.watch();
    /** The part the page scrolls to, until when. @type {{ number: number, until: number } | null} */
    this.reached = null;
  }

  /** Observes the parts' tops crossing the line, which moves with the window's height. */
  watch() {
    this.parts?.disconnect();
    this.ending.disconnect();
    const below = Math.round(window.innerHeight - line());
    this.parts = new IntersectionObserver(() => this.mark(), { rootMargin: `0px 0px -${below}px 0px` });
    for (const section of this.screen.querySelectorAll('.design-part')) this.parts.observe(section);
    removeEventListener('resize', this.onResize);
    addEventListener('resize', this.onResize);
    const end = this.screen.querySelector('.design-end');
    if (end) this.ending.observe(end);
    this.mark();
  }

  /**
   * Marks part `number` as the one in view while the page scrolls to it: a part near the end
   * of the design may not reach the top of the window, and the scroll the page makes to it does
   * not count as the reviewer's.
   * @param {number} number
   */
  reach(number) {
    this.reached = { number, until: performance.now() + 300 };
    this.mark();
  }

  mark() {
    if (!this.screen.isConnected || this.screen.closest('[hidden]')) return;
    const sections = [...this.screen.querySelectorAll('.design-part')];
    let current = 1;
    sections.forEach((section, index) => {
      if (section.getBoundingClientRect().top <= line()) current = index + 1;
    });
    const end = this.screen.querySelector('.design-end');
    const atEnd = end !== null && end.getBoundingClientRect().top <= window.innerHeight;
    if (atEnd && window.scrollY > 0) current = sections.length;
    if (this.reached && performance.now() < this.reached.until) current = this.reached.number;
    this.screen.dataset.part = String(current);
    for (const marked of this.screen.querySelectorAll('[data-part]')) {
      if (!(marked instanceof HTMLElement)) continue;
      const number = Number(marked.dataset.part);
      marked.classList.toggle('current', number === current);
      marked.classList.toggle('read', number < current);
      const link = marked.tagName === 'LI' ? marked.querySelector('a') : null;
      if (link && number === current) link.setAttribute('aria-current', 'location');
      else link?.removeAttribute('aria-current');
    }
  }

  stop() {
    this.parts?.disconnect();
    this.ending.disconnect();
    removeEventListener('resize', this.onResize);
  }
}

/** The line a part's top passes to be the part in view: the upper third of the window, at most
 * 240 pixels down. */
function line() {
  return Math.min(window.innerHeight / 3, 240);
}
