// The page: its screens, the design of the change, each earlier question as the reviewer
// answered it, the answered quiz, and the round's current stage, of which the address shows one
// (route.js); on a phone a swipe turns from one to the next (swipe.js). Each screen is a list of
// regions, in the order the page shows them, drawn from the latest view (dom.js has the rules).
// Each screen or region has its own module; a new part of the page is one more region here, and
// one module.

/** @import { AskedUnder, Call, PageView, Reply, StatusCard } from "./types.ts" */
/** @import { Current } from "./design.js" */

import { Chat } from './chat.js';
import { QuotePicker } from './chat-quote.js';
import { ConclusionScreen } from './conclusion.js';
import { DesignScreen } from './design.js';
import { earlierScreen } from './earlier.js';
import { drawDiagrams, fitDiagrams } from './diagrams.js';
import { Favicon } from './favicon.js';
import { h, keyOf, Region } from './dom.js';
import { Masthead } from './masthead.js';
import { Meter } from './meter.js';
import { Notifier } from './notify.js';
import { QuestionScreen } from './question.js';
import { itemOpens, quizOpens, QuizScreen, railShowing } from './quiz.js';
import { DESIGN, earlierQuestion, openRound, quizItem, route, STAGE } from './route.js';
import { SentScreen } from './sent.js';
import { startCover } from './start.js';
import { statusCard } from './status.js';
import { Swipe } from './swipe.js';
import { turnStrip } from './turn.js';

export class Page {
  /**
   * @param {HTMLElement} main
   * @param {HTMLElement} header the masthead, above `main`
   * @param {(call: Call) => Promise<Reply>} call sends a request on the page's socket
   */
  constructor(main, header, call) {
    this.main = main;
    const redraw = () => {
      if (this.view) this.render(this.view);
    };
    // The notifications the reviewer turns on in the masthead's menu.
    this.notifier = new Notifier(redraw);
    // What only the masthead knows (what it has open) changes the page through `render` too.
    this.masthead = new Masthead(header, redraw, this.notifier);
    this.meter = new Meter(/** @type {HTMLElement} */ (header.querySelector('#masthead-line')));
    // The tab's icon, which shows the meter's share, or the agent at work, in small.
    this.favicon = new Favicon();
    // The round's conversation with the agent, whose bubble sits in the masthead.
    this.chat = new Chat(/** @type {HTMLElement} */ (header.querySelector('#masthead-chat')), call, () => {
      if (this.view) this.render(this.view);
    });
    this.quotes = new QuotePicker(main);
    /** The design screen. */
    this.designScreen = h('div', { class: 'screen', hidden: true });
    /** The round's current stage. */
    this.stage = h('div', { class: 'screen' });
    /** The answered quiz, once the stage shows the conclusion after it. */
    this.quizScreen = h('div', { class: 'screen', hidden: true });
    main.append(this.designScreen, this.stage, this.quizScreen);
    /** The screen of each earlier question, by its number, after the stage: the stage's diagrams
     * keep their numbers. @type {Map<number, { screen: HTMLElement, region: Region }>} */
    this.earlierScreens = new Map();
    this.design = new Region(this.designScreen, 'design');
    this.cards = new Region(this.stage, 'cards');
    this.start = new Region(this.stage, 'start');
    this.turn = new Region(this.stage, 'turn');
    this.sent = new Region(this.stage, 'sent');
    this.question = new Region(this.stage, 'question');
    this.conclusion = new Region(this.stage, 'conclusion');
    this.answeredQuiz = new Region(this.quizScreen, 'answered-quiz');
    /** The item the answered quiz shows, from 0, which its address names while it shows. */
    this.answeredItem = 0;
    /** The screen the page shows, and the part of the design it shows. @type {string | null} */
    this.shown = null;
    /** The screen the page shows other than the stage, which the rail shows as the step in view.
     * @type {import('./masthead.js').Viewing} */
    this.viewed = null;
    /** Where the stage was scrolled to when another screen opened. */
    this.stageScroll = 0;
    /** Where a swipe left the top of the screen it turned to, in the window, until it shows.
     * @type {number | null} */
    this.swipedTo = null;
    /** @type {DesignScreen | null} */
    this.shownDesign = null;
    addEventListener('hashchange', () => {
      if (this.view) this.render(this.view);
    });
    this.swipe = new Swipe(main, {
      track: () => this.track(),
      settled: (address, top) => {
        if (address !== null) {
          this.swipedTo = top;
          location.hash = address;
        }
        // What changed while the swipe moved the screens shows now.
        if (this.view) this.render(this.view);
      },
      chip: (step, on) => this.masthead.target(step, on),
    });
    /** @type {PageView | null} */
    this.view = null;
    /** Why the reviewer's latest action did not go through, until the round changes.
     * @type {StatusCard | null} */
    this.notice = null;
    /** The quiz item the reviewer just answered or moved to while the quiz asks, which shows
     * until the reviewer moves on; after the last item, until the reviewer shows the conclusion.
     * @type {import('./quiz.js').QuizAt | null} */
    this.quizAt = null;
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
    const quizShown = this.renderConclusion(view);
    // The question, the quiz and the conclusion show the previous turn on their own desk, and a
    // turn that carries the reviewer's answer shows that answer in its panel; any other stage,
    // above it.
    const turn = turnOf(view);
    this.turn.show(view.question || view.conclusion || view.sent ? null : keyOf(turn), () => turnStrip(turn));
    this.renderSent(view);
    this.renderScreens(view, quizShown !== null);
    this.chat.update(view, chatPlace(view, this.viewed));
    this.quotes.update(view.conversation?.writable ?? false);
    // While the quiz shows an item, the rail names it.
    const quiz = view.conclusion?.quiz;
    const rail = quiz && quizShown !== null ? railShowing(view.rail, quizShown, quiz.items.length) : view.rail;
    this.masthead.update(rail === view.rail ? view : { ...view, rail }, this.viewed, this.chat.unread(), this.quizLink());
    const { screens, shown } = this.track();
    this.masthead.neighbours([screens[shown - 1]?.step, screens[shown + 1]?.step]);
    this.meter.update(view);
    this.favicon.update(view);
    this.notifier.update(view);
    drawDiagrams(this.main);
  }

  /** The design screen, the earlier questions' screens, the answered quiz, and which screen shows.
   * @param {PageView} view
   * @param {boolean} asking whether the stage shows the quiz, which asks for answers */
  renderScreens(view, asking) {
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
    this.renderEarlier(view, current);
    const answered = this.renderAnsweredQuiz(view, asking);
    // A swipe under way moves the screens; the page shows the address's screen once it ends.
    if (this.swipe.active) return;
    const asked = route();
    const showsDesign = asked.screen === 'design' && design !== null;
    const earlier = asked.screen === 'question' ? this.earlierScreens.get(asked.number) : undefined;
    // An earlier question the round no longer has (its answer was cancelled) gives way to the
    // stage, and so does its address, which would otherwise open it again once it is back.
    // So does the answered quiz while the quiz still asks, or once the round has none.
    if ((asked.screen === 'question' && !earlier) || (asked.screen === 'quiz' && !answered)) {
      history.replaceState(history.state, '', STAGE);
    }
    const showsQuiz = asked.screen === 'quiz' && answered;
    this.designScreen.hidden = !showsDesign;
    for (const { screen } of this.earlierScreens.values()) screen.hidden = screen !== earlier?.screen;
    this.quizScreen.hidden = !showsQuiz;
    this.stage.hidden = showsDesign || earlier !== undefined || showsQuiz;
    // Scroll only when the screen, or the part of the design the address names, changed: the
    // quiz scrolls to the item it moves to by itself.
    const shown = showsDesign
      ? `design:${asked.screen === 'design' ? asked.part : ''}`
      : earlier && asked.screen === 'question'
        ? `question:${asked.number}`
        : showsQuiz
          ? 'quiz'
          : 'stage';
    if (shown === this.shown) return;
    // The reviewer who opens another screen from the stage comes back to where they were.
    if (this.shown === 'stage') this.stageScroll = window.scrollY;
    this.shown = shown;
    this.viewed = showsDesign
      ? { kind: 'design' }
      : earlier && asked.screen === 'question'
        ? { kind: 'question', number: asked.number }
        : showsQuiz
          ? { kind: 'quiz' }
          : null;
    fitDiagrams();
    const swiped = this.swipedTo;
    this.swipedTo = null;
    if (swiped !== null) {
      // A swipe showed the screen's top where it now is: the page keeps it there.
      const screen = showsDesign ? this.designScreen : showsQuiz ? this.quizScreen : (earlier?.screen ?? this.stage);
      window.scrollTo(0, window.scrollY + screen.getBoundingClientRect().top - swiped);
    } else if (showsDesign) this.shownDesign?.scrollTo(asked.screen === 'design' ? asked.part : null);
    else if (earlier || showsQuiz) window.scrollTo(0, 0);
    else scrollToFragment(this.stageScroll);
  }

  /** Builds the screen of each earlier question, each rebuilt only when its data changes.
   * @param {PageView} view
   * @param {Current | null} current */
  renderEarlier(view, current) {
    const numbers = new Set(view.earlier_questions.map((question) => question.number));
    for (const [number, { screen }] of this.earlierScreens) {
      if (numbers.has(number)) continue;
      screen.remove();
      this.earlierScreens.delete(number);
    }
    for (const question of view.earlier_questions) {
      let earlier = this.earlierScreens.get(question.number);
      if (!earlier) {
        const screen = h('div', { class: 'screen', hidden: true });
        this.main.append(screen);
        earlier = { screen, region: new Region(screen, `question-${question.number}`) };
        this.earlierScreens.set(question.number, earlier);
      }
      earlier.region.show(keyOf([question, current]), () => earlierScreen(question, current));
    }
  }

  /**
   * Builds the answered quiz, read only, at the item its address names, once the stage shows the
   * conclusion after the quiz.
   * @param {PageView} view
   * @param {boolean} asking whether the stage shows the quiz, which asks for answers
   * @returns {boolean} whether the page has the answered quiz
   */
  renderAnsweredQuiz(view, asking) {
    const conclusion = view.conclusion;
    const quiz = conclusion?.quiz;
    if (!conclusion || !quiz || quiz.items.length === 0 || asking) {
      this.answeredQuiz.clear();
      this.answeredItem = 0;
      return false;
    }
    const asked = route();
    if (asked.screen === 'quiz') {
      this.answeredItem = Math.min(asked.item, quiz.items.length) - 1;
      // An item the quiz does not have gives way to its last one.
      if (asked.item > quiz.items.length) history.replaceState(history.state, '', quizItem(this.answeredItem + 1));
    }
    this.answeredItem = Math.min(this.answeredItem, quiz.items.length - 1);
    this.answeredQuiz
      .component(`quiz:${conclusion.request}`, () => new QuizScreen())
      .update(quiz, conclusion.request, turnOf(view), {
        shown: this.answeredItem,
        asking: false,
        open: (item) => {
          location.hash = quizItem(item + 1);
        },
        done: () => {},
      });
    return true;
  }

  /** Whether the rail's quiz step opens the answered quiz: the page has it, which it has only
   * while the stage shows the conclusion, and the reviewer answered at least one item. */
  quizLink() {
    const step = this.view?.rail.find((each) => each.step.kind === 'quiz');
    return this.answeredQuiz.current !== undefined && step !== undefined && quizOpens(step);
  }

  /** The screens a swipe turns between, in the rail's order, and the one that shows.
   * @returns {import('./swipe.js').Track} */
  track() {
    /** @type {import('./swipe.js').Screen[]} */
    const screens = [];
    if (this.view?.design) screens.push({ address: DESIGN, element: this.designScreen, step: 'design' });
    const numbers = [...this.earlierScreens.keys()].sort((a, b) => a - b);
    for (const number of numbers) {
      const { screen } = /** @type {{ screen: HTMLElement }} */ (this.earlierScreens.get(number));
      screens.push({ address: earlierQuestion(number), element: screen, step: `question-${number}` });
    }
    // The answered quiz, where its step on the rail opens it, or while it shows.
    const viewed = this.viewed;
    if (this.quizLink() || (this.answeredQuiz.current && viewed?.kind === 'quiz')) {
      screens.push({ address: quizItem(this.answeredItem + 1), element: this.quizScreen, step: 'quiz' });
    }
    screens.push({ address: STAGE, element: this.stage, step: 'round' });
    // From what the page shows, rather than from which screens are hidden: a swipe shows the
    // neighbours while it moves.
    const step =
      viewed === null ? 'round' : viewed.kind === 'question' ? `question-${viewed.number}` : viewed.kind;
    return { screens, shown: Math.max(screens.findIndex((screen) => screen.step === step), 0) };
  }

  /** The answer the agent's turn carries, beside the turn's card.
   * @param {PageView} view */
  renderSent(view) {
    const sent = view.sent;
    if (!sent) {
      this.sent.clear();
      return;
    }
    this.sent.component(`sent:${view.reset ?? ''}`, () => new SentScreen()).update(sent, view.cancellable);
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

  /** The conclusion, or its quiz first: the item the reviewer just answered or moved to, or the
   * next one to answer.
   * @param {PageView} view
   * @returns {number | null} the quiz item the page shows, from 0, if any */
  renderConclusion(view) {
    const conclusion = view.conclusion;
    if (!conclusion) {
      this.conclusion.clear();
      return null;
    }
    const quiz = conclusion.quiz;
    const request = conclusion.request;
    const at = this.quizAt;
    const moved = quiz && at?.conclusion === request && itemOpens(quiz, at.item, true) ? at.item : null;
    const shown = quiz ? (moved ?? quiz.next) : null;
    if (quiz && shown !== null) {
      /** @param {import('./quiz.js').QuizAt | null} where */
      const go = (where) => {
        this.quizAt = where;
        this.render(/** @type {PageView} */ (this.view));
      };
      this.conclusion
        .component(`quiz:${request}`, () => new QuizScreen())
        .update(quiz, request, turnOf(view), {
          shown,
          asking: true,
          open: (item) => go({ conclusion: request, item }),
          done: () => go(null),
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

/**
 * Where the page shows the chat, which a message is written under: the design, an earlier
 * question, the question with its version, or the conclusion; nothing in another stage.
 * @param {PageView} view
 * @param {import('./masthead.js').Viewing} viewed the screen the page shows, other than the stage
 * @returns {import('./chat.js').Place}
 */
function chatPlace(view, viewed) {
  if (viewed?.kind === 'design') return { askedUnder: { stage: 'design' }, question: false };
  if (viewed?.kind === 'question') {
    const earlier = view.earlier_questions.find((question) => question.number === viewed.number);
    const askedUnder = earlier ? { stage: /** @type {const} */ ('question'), question: earlier.id, version: earlier.version } : null;
    return { askedUnder, question: false };
  }
  /** @type {AskedUnder | null} */
  let askedUnder = null;
  if (view.question) askedUnder = { stage: 'question', question: view.question.id, version: view.question.version };
  else if (view.conclusion) askedUnder = { stage: 'conclusion', conclusion: view.conclusion.request };
  return { askedUnder, question: view.question !== null };
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

/** The previous turn of the view: the reviewer's latest answer, which can be cancelled until an
 * implementation request is made, and what the agent's turn said back to it. An answer that can
 * no longer be cancelled still shows, as the round kept it.
 * @param {PageView} view
 * @returns {import('./turn.js').Turn} */
function turnOf(view) {
  const kept = view.cancellable
    ? null
    : (view.earlier_questions.find((question) => question.number === view.answered)?.answer ?? null);
  return { answer: view.cancellable, kept, response: view.response, number: view.answered };
}
