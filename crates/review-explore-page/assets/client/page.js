// The page: one region of `main` for each part of a screen, in the order the page shows them,
// drawn from the latest view (dom.js has the rules). Each screen or region has its own module;
// a new part of the page is one more region here, and one module.

/** @import { PageView, StatusCard } from "./types.ts" */

import { ConclusionScreen } from './conclusion.js';
import { designSection } from './design.js';
import { drawDiagrams } from './diagrams.js';
import { keyOf, Region } from './dom.js';
import { lastAnswer } from './last-answer.js';
import { Masthead } from './masthead.js';
import { QuestionScreen } from './question.js';
import { quizSection } from './quiz.js';
import { responseSection } from './response.js';
import { startCover } from './start.js';
import { statusCard } from './status.js';

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
    this.cards = new Region(main, 'cards');
    this.start = new Region(main, 'start');
    this.lastAnswer = new Region(main, 'last-answer');
    this.design = new Region(main, 'design');
    this.response = new Region(main, 'response');
    this.question = new Region(main, 'question');
    this.conclusion = new Region(main, 'conclusion');
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
    this.masthead.update(view);
    const cards = this.notice ? [this.notice, ...view.cards] : view.cards;
    this.cards.show(keyOf(cards), () => cards.map(statusCard));
    const start = view.start;
    this.start.show(keyOf([start, view.review]), () => (start ? startCover(start, view.review) : null));
    const answer = view.cancellable;
    this.lastAnswer.show(keyOf(answer), () => (answer ? lastAnswer(answer) : null));
    const design = view.design;
    this.design.show(keyOf(design), () => (design ? designSection(design) : null));
    const response = view.response;
    this.response.show(keyOf(response), () => (response ? responseSection(response) : null));
    this.renderQuestion(view);
    this.renderConclusion(view);
    drawDiagrams(this.main);
  }

  /** @param {PageView} view */
  renderQuestion(view) {
    const question = view.question;
    if (!question) {
      this.question.clear();
      return;
    }
    const key = keyOf([question.round, question.id, question.version, question.number]);
    this.question.component(key, () => new QuestionScreen(question)).update(question);
  }

  /** The conclusion, or its quiz first: the item the reviewer just answered, or the next one.
   * @param {PageView} view */
  renderConclusion(view) {
    const conclusion = view.conclusion;
    if (!conclusion) {
      this.conclusion.clear();
      return;
    }
    const quiz = conclusion.quiz;
    const answered =
      this.answered?.conclusion === conclusion.request &&
      quiz?.items[this.answered.item]?.picked_correct !== null
        ? this.answered.item
        : null;
    const shown = quiz ? (answered ?? quiz.next) : null;
    if (quiz && shown !== null) {
      this.conclusion.show(keyOf(['quiz', quiz, conclusion.request, shown]), () =>
        quizSection(quiz, conclusion.request, shown, () => {
          this.answered = null;
          this.render(/** @type {PageView} */ (this.view));
        }),
      );
      return;
    }
    this.conclusion
      .component(`conclusion:${conclusion.request}`, () => new ConclusionScreen())
      .update(conclusion, view.reset);
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
