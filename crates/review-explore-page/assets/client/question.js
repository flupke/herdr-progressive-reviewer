// The question screen (docs/design/explore-page/README.md, "2. Question screen"): on the desk,
// the previous turn, the question as the headline with its chips, its Context, Door and Blast
// radius as rows that open, then its citations; in the panel, the reviewer's answer.
//
// On a question whose Door is not two-way, the pick is blind: the panel hides the agent's
// recommendation, and the tool does not even send it, until the reviewer's first Send. That Send
// records the first pick (`pick`) and sends no answer; the panel then shows the recommendation,
// with a line that says whether the reviewer and the agent picked the same choice, and its
// button becomes Confirm answer, which sends the answer (`answer`). The comment typed before the
// first Send stays in the comment box, since both forms keep the same draft.
//
// The section stays while the page shows the same version of the question; its parts (the turn,
// the reading, the panel, the citations) are rebuilt only when their own data changes, so a
// first pick rebuilds the panel alone and leaves the reader's folds and scroll as they are.

/** @import { Door, QuestionView, ChoiceView, MarksView, GainView, SectionView } from "./types.ts" */
/** @import { Children } from "./dom.js" */
/** @import { Turn } from "./turn.js" */

import { DOORS, doorChip } from './chips.js';
import { choiceCards } from './choices.js';
import { citationsSection } from './citations.js';
import { disclosure } from './disclosure.js';
import { h, keyOf, markdown, Region, setRenderedMarkdown } from './dom.js';
import { keepDraft } from './drafts.js';
import { turnStrip } from './turn.js';

export class QuestionScreen {
  /** @param {QuestionView} question */
  constructor(question) {
    this.element = h('section', {
      class: 'question desk',
      // What Cancel answer brings back into view (CHANGED in actions.js).
      'data-shows': 'cancel-answer',
      'aria-labelledby': `question-${question.number}-label`,
      'data-question': question.id,
      'data-version': question.version,
    });
    this.turn = new Region(this.element, 'turn');
    this.reading = new Region(this.element, 'reading');
    this.panel = new Region(this.element, 'panel');
    this.citations = new Region(this.element, 'citations');
  }

  /**
   * @param {QuestionView} question
   * @param {Turn} turn the turn that led to the question
   */
  update(question, turn) {
    this.turn.show(keyOf(turn), () => turnStrip(turn));
    // The first pick of a blind question changes the panel only: the reading keeps its folds.
    const { text_html, context_html, sections, number, door, answerable } = question;
    const blind = question.recommendation !== 'shown';
    this.reading.show(keyOf({ text_html, context_html, sections, number, door, blind, answerable }), () =>
      reading(question),
    );
    this.panel.show(
      keyOf([question.answerable, question.recommendation, question.choices, question.marks, question.gain, door]),
      () => (question.answerable ? panel(question) : null),
    );
    this.citations.show(keyOf(question.citations), () => citationsSection(question.citations, question.number));
  }
}

/** The question's head, its Context, then Door and Blast radius, folded to their leads. "Blind
 * pick" follows the door's chip while the question is blind; on a phone, a link leads past the
 * reading to the choices.
 * @param {QuestionView} question */
function reading(question) {
  return questionReading(
    { ...question, label: `question-${question.number}-label`, name: `Question ${question.number}` },
    [
      question.recommendation !== 'shown' ? h('span', { class: 'chip agent' }, 'Blind pick') : null,
      question.answerable ? h('a', { class: 'choices-link', href: '#answer' }, 'Choices ↓') : null,
    ],
  );
}

/**
 * What the reader reads of a question, on the question screen and on an earlier question's: the
 * eyebrow that names it ("QUESTION 2"), with the door's chip and `more`, the question as the
 * headline, then its Context, and Door and Blast radius folded to their leads.
 * @param {{ label: string, name: string, door: Door | null, text_html: string,
 *   context_html: string | null, sections: SectionView[] }} question `label` is the ID of the
 *   eyebrow's name, which names the question's region
 * @param {Children} more what follows the door's chip in the eyebrow
 */
export function questionReading(question, more) {
  const headline = h('h2', { class: 'question-text' });
  setRenderedMarkdown(headline, question.text_html);
  return [
    h(
      'header',
      { class: 'question-head' },
      h(
        'p',
        { class: 'eyebrow question-eyebrow' },
        h('span', { id: question.label }, question.name),
        question.door ? doorChip(question.door) : null,
        more,
      ),
      headline,
    ),
    question.context_html !== null ? markdown(question.context_html, 'explanation') : null,
    question.sections.length > 0 ? h('div', { class: 'assessments' }, question.sections.map(assessment)) : null,
  ].filter((node) => node !== null);
}

/** A section of the question folded to one row, its lead beside its title; open, the lead wraps
 * and the rest follows.
 * @param {SectionView} section */
function assessment(section) {
  const lead = h('span', { class: 'assessment-lead' });
  setRenderedMarkdown(lead, section.lead_html);
  const { element } = disclosure(
    [h('span', { class: 'assessment-title' }, section.title), lead],
    section.details_html !== null ? markdown(section.details_html) : [],
    { button: 'disclosure-row assessment-toggle' },
  );
  element.classList.add('assessment');
  return element;
}

/** The reviewer's answer: the choices, an optional comment, what the answer marks, and Send
 * answer; Confirm answer once the first Send of a blind question revealed the recommendation.
 * @param {QuestionView} question */
function panel(question) {
  const picking = question.recommendation === 'hidden_until_pick';
  const revealed = question.recommendation === 'shown_after_pick';
  const door = question.door ? DOORS[question.door].name : 'This question';
  const draft = `comment:${question.round ?? ''}:${question.id}:${question.version}`;
  return h(
    'form',
    {
      class: 'answer panel',
      id: 'answer',
      'aria-label': `Your answer to question ${question.number}`,
      'data-method': picking ? 'pick' : 'answer',
      'data-requires': picking ? 'choice' : 'choice-or-comment',
    },
    question.round !== null ? h('input', { type: 'hidden', name: 'round', value: question.round }) : null,
    h('input', { type: 'hidden', name: 'question', value: question.id }),
    h('input', { type: 'hidden', name: 'version', value: question.version }),
    // The number a refusal names the question by.
    question.number > 0 ? h('input', { type: 'hidden', name: 'number', value: question.number }) : null,
    picking
      ? h(
          'p',
          { class: 'hint' },
          `${door}: the agent's recommendation shows when you send, so your first read of the choices is your own. You can still change your pick after.`,
        )
      : null,
    revealed ? revealLine(question.choices) : null,
    choiceCards(question.choices, { picking, revealed }),
    h(
      'label',
      { class: 'eyebrow comment-label', for: 'comment' },
      'Comment',
      h('span', { class: 'optional' }, ' · optional'),
    ),
    keepDraft(h('textarea', { class: 'comment', id: 'comment', name: 'comment', rows: 3 }), draft, ''),
    question.marks ? gainLine(question.marks, question.gain) : null,
    h('button', { class: 'button primary block', type: 'submit' }, revealed ? 'Confirm answer' : 'Send answer'),
  );
}

/** The line at the top of the panel once the recommendation shows after the first pick: whether
 * the reviewer and the agent picked the same choice, the checked one. Disagreeing is
 * information, not an error. The page brings it into view after the first pick.
 * @param {ChoiceView[]} choices */
function revealLine(choices) {
  const same = choices.some((choice) => choice.checked && choice.recommendation !== null);
  const shows = { tabindex: -1, 'data-shows': 'pick' };
  return same
    ? h('p', { class: 'reveal same', ...shows }, h('strong', {}, 'You and the agent picked the same choice.'))
    : h(
        'p',
        { class: 'reveal other', ...shows },
        h('strong', {}, 'The agent recommends another choice.'),
        ' Read its reason, then keep yours or change it.',
      );
}

/** What answering marks, and the reviewed share of the change before and after, with its bar;
 * the lines on request.
 * @param {MarksView} marks
 * @param {GainView | null} gain */
function gainLine(marks, gain) {
  const [first, ...rest] = marks.summary.parts;
  const { element } = disclosure(
    [
      h(
        'span',
        { class: 'gain-line' },
        h('span', { class: 'gain-text' }, `${marks.summary.verb} `, h('strong', {}, first), rest.map((part) => ` · ${part}`)),
        gain ? h('span', { class: 'gain-share' }, `${gain.before}% → `, h('strong', {}, `${gain.after}%`)) : null,
      ),
      gain ? gainBar(gain) : null,
    ],
    h(
      'ul',
      { class: 'gain-lines' },
      marks.reviewed.map((line) => h('li', {}, `${line} (reviewed)`)),
      marks.not_relevant.map((line) => h('li', {}, line)),
      marks.reopened.map((line) => h('li', {}, `${line} (reopened)`)),
    ),
    { button: 'disclosure-row gain-toggle' },
  );
  element.classList.add('gain');
  return element;
}

/** The share reviewed now, then the share the answer adds, hatched.
 * @param {GainView} gain */
function gainBar(gain) {
  const done = h('span', { class: 'gain-done' });
  const added = h('span', { class: 'gain-added' });
  done.style.width = `${gain.before}%`;
  added.style.width = `${Math.max(gain.after - gain.before, 0)}%`;
  return h('span', { class: 'gain-bar', 'aria-hidden': 'true' }, done, added);
}
