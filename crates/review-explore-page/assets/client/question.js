// The agent's question: its text and explanation on the desk, the reviewer's answer in the
// panel, then its citations (layout.css places them). On a question whose Door is not two-way,
// the panel asks for the first pick before it shows the agent's recommendation; the comment
// typed with the pick stays in the comment box, since both forms keep the same draft.
//
// The section stays while the page shows the same version of the question; its three parts
// (the reading, the panel, the citations) are rebuilt only when their own data changes, so a
// pick rebuilds the panel alone and leaves the reader's folds and scroll as they are.

/** @import { QuestionView, ChoiceView, MarksView } from "./types.ts" */

import { citationsSection } from './citations.js';
import { h, keyOf, markdown, Region } from './dom.js';
import { keepDraft } from './drafts.js';

export class QuestionScreen {
  /** @param {QuestionView} question */
  constructor(question) {
    this.element = h('section', {
      class: 'question desk',
      'aria-labelledby': `question-${question.number}-title`,
      'data-question': question.id,
      'data-version': question.version,
    });
    this.reading = new Region(this.element, 'reading');
    this.panel = new Region(this.element, 'panel');
    this.citations = new Region(this.element, 'citations');
  }

  /** @param {QuestionView} question */
  update(question) {
    const { text_html, context_html, sections, number } = question;
    this.reading.show(keyOf({ text_html, context_html, sections, number }), () => reading(question));
    this.panel.show(
      keyOf([question.answerable, question.recommendation, question.choices, question.first_pick, question.marks]),
      () => (question.answerable ? panel(question) : null),
    );
    this.citations.show(keyOf(question.citations), () => citationsSection(question.citations, question.number));
  }
}

/** The question's heading, its text, and its explanation: Context, then Door and Blast radius,
 * folded away until the reviewer opens them.
 * @param {QuestionView} question */
function reading(question) {
  return [
    h('h2', { id: `question-${question.number}-title` }, `Question ${question.number}`),
    markdown(question.text_html, 'text'),
    question.context_html !== null ? markdown(question.context_html, 'explanation') : null,
    ...question.sections.map((section) =>
      h('details', { class: 'assessment' }, h('summary', {}, section.title), markdown(section.body_html)),
    ),
  ].filter((node) => node !== null);
}

/** The reviewer's answer: a choice, an optional comment, and Send; or, before the first pick of a
 * blind question, the pick.
 * @param {QuestionView} question */
function panel(question) {
  const picking = question.recommendation === 'hidden_until_pick';
  const draft = `comment:${question.round ?? ''}:${question.id}:${question.version}`;
  return h(
    'form',
    picking
      ? { class: 'answer pick panel', 'data-method': 'pick' }
      : { class: 'answer panel', 'data-method': 'answer' },
    question.round !== null ? h('input', { type: 'hidden', name: 'round', value: question.round }) : null,
    h('input', { type: 'hidden', name: 'question', value: question.id }),
    h('input', { type: 'hidden', name: 'version', value: question.version }),
    picking
      ? h(
          'p',
          { class: 'hint' },
          "Pick your answer before you see the agent's recommendation: it shows once you have picked.",
        )
      : null,
    !picking && question.first_pick !== null
      ? h('p', { class: 'hint' }, `You picked “${question.first_pick}” first. Keep your pick or change it, then send.`)
      : null,
    choices(question.choices, picking),
    h('label', { class: 'comment-label', for: 'comment' }, 'Comment (optional)'),
    keepDraft(h('textarea', { class: 'comment', id: 'comment', name: 'comment', rows: 3 }), draft, ''),
    !picking && question.marks ? marks(question.marks) : null,
    h('button', { class: 'button primary block', type: 'submit' }, picking ? 'Pick' : 'Send'),
  );
}

/** The choices: on a blind question the first pick is required. A recommended choice is
 * described by the agent's reason, unless the panel hides it until the first pick.
 * @param {ChoiceView[]} choices
 * @param {boolean} picking */
function choices(choices, picking) {
  return h(
    'fieldset',
    { class: 'choices' },
    h('legend', {}, 'Choices'),
    choices.map((choice, index) => {
      const described = choice.recommendation !== null ? `recommendation-${index + 1}` : null;
      return [
        h(
          'label',
          { class: choice.recommendation !== null ? 'choice recommended' : 'choice' },
          h('input', {
            type: 'radio',
            name: 'choice',
            value: choice.id,
            checked: choice.checked,
            required: picking,
            'aria-describedby': described,
          }),
          ` ${choice.text}`,
        ),
        choice.recommendation !== null
          ? h(
              'p',
              { class: 'recommendation', id: described },
              h('strong', {}, 'Recommended:'),
              ` ${choice.recommendation}`,
            )
          : null,
      ];
    }),
  );
}

/** What an answer to the question marks, before the reviewer sends it; the lines on request.
 * @param {MarksView} marks */
function marks(marks) {
  return h(
    'details',
    { class: 'marks' },
    h('summary', {}, `${marks.summary} when you answer`),
    h(
      'ul',
      {},
      marks.reviewed.map((line) => h('li', {}, `${line} (reviewed)`)),
      marks.not_relevant.map((line) => h('li', {}, line)),
      marks.reopened.map((line) => h('li', {}, `${line} (reopened)`)),
    ),
  );
}
