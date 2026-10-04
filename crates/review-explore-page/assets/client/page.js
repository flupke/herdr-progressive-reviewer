// The page: two screens, the design of the change and the round's current stage, of which the
// address shows one (route.js). Each screen is a list of regions, in the order the page shows
// them, drawn from the latest view (dom.js has the rules). Each screen or region has its own
// module; a new part of the page is one more region here, and one module.

/** @import { PageView, StatusCard } from "./types.ts" */
/** @import { Current } from "./design.js" */

import { ConclusionScreen } from './conclusion.js';
import { DesignScreen } from './design.js';
import { drawDiagrams, fitDiagrams } from './diagrams.js';
import { h, keyOf, Region } from './dom.js';
import { Masthead } from './masthead.js';
import { Meter } from './meter.js';
import { QuestionScreen } from './question.js';
import { QuizScreen, railShowing } from './quiz.js';
import { openRound, route, STAGE } from './route.js';
import { startCover } from './start.js';
import { statusCard } from './status.js';
import { turnStrip } from './turn.js';

export class Page {
  /**
   * @param {HTMLElement} main
   * @param {HTMLElement} header the masthead, above `main`
   */
  constructor(main, header) {
    this.main = main;
    // What only the masthead knows (what it has open) changes the page through `render` too.
    this.masthead = new Masthead(header, () => {
      if (this.view) this.render(this.view);
    });
    this.meter = new Meter(/** @type {HTMLElement} */ (header.querySelector('#masthead-line')));
    /** The design screen. */
    this.designScreen = h('div', { class: 'screen', hidden: true });
    /** The round's current stage. */
    this.stage = h('div', { class: 'screen' });
    main.append(this.designScreen, this.stage);
    this.design = new Region(this.designScreen, 'design');
    this.cards = new Region(this.stage, 'cards');
    this.start = new Region(this.stage, 'start');
    this.turn = new Region(this.stage, 'turn');
    this.question = new Region(this.stage, 'question');
    this.conclusion = new Region(this.stage, 'conclusion');
    /** The screen the page shows, and the part of the design it shows. @type {string | null} */
    this.shown = null;
    /** Where the stage was scrolled to when the design screen opened. */
    this.stageScroll = 0;
    /** @type {DesignScreen | null} */
    this.shownDesign = null;
    addEventListener('hashchange', () => {
      if (this.view) this.render(this.view);
    });
    /** @type {PageView | null} */
    this.view = null;
    /** Why the reviewer's latest action did not go through, until the round changes.
     * @type {StatusCard | null} */
    this.notice = null;
    /** The quiz item the reviewer just answered, which shows until the reviewer moves on.
     * @type {{ conclusion: string, item: number } | null} */
    this.answered = null;
  }

  /** @param {PageView} view */
  render(view) {
    this.view = view;
    const cards = this.notice ? [this.notice, ...view.cards] : view.cards;
    this.cards.show(keyOf(cards), () => cards.map(statusCard));
    const start = view.start;
    const tally = view.tally;
    this.start.show(keyOf([start, view.review, tally?.change.changed, tally?.files.length]), () =>
      start ? startCover(start, view.review, tally) : null,
    );
    this.renderQuestion(view);
    const quizItem = this.renderConclusion(view);
    // The question, the quiz and the conclusion show the previous turn on their own desk; any
    // other stage, above it.
    const turn = turnOf(view);
    this.turn.show(view.question || view.conclusion ? null : keyOf(turn), () => turnStrip(turn));
    this.renderDesign(view);
    // While the quiz shows an item, the rail names it.
    const quiz = view.conclusion?.quiz;
    const rail = quiz && quizItem !== null ? railShowing(view.rail, quizItem, quiz.items.length) : view.rail;
    this.masthead.update(rail === view.rail ? view : { ...view, rail }, !this.designScreen.hidden);
    this.meter.update(view);
    drawDiagrams(this.main);
  }

  /** The design screen, and which of the two screens shows.
   * @param {PageView} view */
  renderDesign(view) {
    const current = currentStep(view);
    openRound(view, current?.kind === 'question' && !current.working ? current.number : null);
    const design = view.design;
    if (design) {
      const screen = this.design.component('design', () => new DesignScreen());
      if (screen !== this.shownDesign) this.shownDesign?.stop();
      this.shownDesign = screen;
      screen.update(design, current, view.question);
    } else {
      this.shownDesign?.stop();
      this.shownDesign = null;
      this.design.clear();
    }
    const asked = route();
    const showsDesign = asked.design && design !== null;
    this.designScreen.hidden = !showsDesign;
    this.stage.hidden = showsDesign;
    // Scroll only when the screen, or the part of the design the address names, changed.
    const shown = showsDesign ? `design:${asked.design ? asked.part : ''}` : 'stage';
    if (shown === this.shown) return;
    // The reviewer who opens the design from the stage comes back to where they were.
    if (this.shown === 'stage') this.stageScroll = window.scrollY;
    this.shown = shown;
    fitDiagrams();
    if (showsDesign) this.shownDesign?.scrollTo(asked.design ? asked.part : null);
    else scrollToFragment(this.stageScroll);
  }

  /** @param {PageView} view */
  renderQuestion(view) {
    const question = view.question;
    if (!question) {
      this.question.clear();
      return;
    }
    const key = keyOf([question.round, question.id, question.version, question.number]);
    this.question.component(key, () => new QuestionScreen(question)).update(question, turnOf(view));
  }

  /** The conclusion, or its quiz first: the item the reviewer just answered, or the next one.
   * @param {PageView} view
   * @returns {number | null} the quiz item the page shows, from 0, if any */
  renderConclusion(view) {
    const conclusion = view.conclusion;
    if (!conclusion) {
      this.conclusion.clear();
      return null;
    }
    const quiz = conclusion.quiz;
    const answered =
      this.answered?.conclusion === conclusion.request &&
      quiz?.items[this.answered.item]?.picked_correct !== null
        ? this.answered.item
        : null;
    const shown = quiz ? (answered ?? quiz.next) : null;
    if (quiz && shown !== null) {
      this.conclusion
        .component(`quiz:${conclusion.request}`, () => new QuizScreen())
        .update(quiz, conclusion.request, shown, turnOf(view), () => {
          this.answered = null;
          this.render(/** @type {PageView} */ (this.view));
        });
      return shown;
    }
    this.conclusion
      .component(`conclusion:${conclusion.request}`, () => new ConclusionScreen())
      .update(conclusion, view.reset, turnOf(view));
    return null;
  }

  /**
   * Shows why the reviewer's latest action did not go through, until the round changes or the
   * reviewer acts again.
   * @param {StatusCard | null} notice
   */
  showNotice(notice) {
    this.notice = notice;
    if (this.view) this.render(this.view);
  }
}

/** The step the round stands at, from the rail; `null` when no round is running.
 * @param {PageView} view
 * @returns {Current | null} */
function currentStep(view) {
  for (const { step, state } of view.rail) {
    if (state.kind !== 'current') continue;
    switch (step.kind) {
      case 'question':
        return { kind: 'question', number: step.number, working: state.working };
      case 'quiz':
        return { kind: 'quiz' };
      case 'conclusion':
        return { kind: 'conclusion' };
      default:
        return null;
    }
  }
  return null;
}

/** Shows what the address's fragment names on the stage, or else scrolls the page to `top`.
 * @param {number} top */
function scrollToFragment(top) {
  const named = location.hash.length > 1 && location.hash !== STAGE;
  const target = named ? document.getElementById(location.hash.slice(1)) : null;
  if (target && !target.closest('[hidden]')) target.scrollIntoView();
  else window.scrollTo(0, top);
}

/** The previous turn of the view: the reviewer's latest answer, while it can be cancelled, and
 * what the agent's turn said back to it.
 * @param {PageView} view
 * @returns {import('./turn.js').Turn} */
function turnOf(view) {
  return { answer: view.cancellable, response: view.response, number: view.answered };
}
