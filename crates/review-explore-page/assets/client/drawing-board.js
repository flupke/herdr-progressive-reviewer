// Back to the drawing board: in place of an answer to the question that waits, the agent stops
// asking and lists the changes the design needs, which the reviewer edits and implements on the
// conclusion. The question's panel and the design screen offer it as a button that a first click
// turns into its red confirmation, with what follows under it, and Escape turns back; the
// masthead's menu shows the same confirmation in place of its entries. All send the same request.

/** @import { QuestionView } from "./types.ts" */

import { h } from './dom.js';

const HINT =
  "The agent stops asking and lists the changes the design needs, for you to edit on the conclusion. Acting on the conclusion reopens the lines this round marked reviewed, for a new round on the new code; until then, Cancel answer takes it back.";

const CONFIRM = 'Confirm: back to the drawing board';

/** The hidden fields that name the question an answer answers, in the answer's form and in
 * Back to the drawing board's.
 * @param {QuestionView} question */
export function answerFields(question) {
  return [
    question.round !== null ? h('input', { type: 'hidden', name: 'round', value: question.round }) : null,
    h('input', { type: 'hidden', name: 'question', value: question.id }),
    h('input', { type: 'hidden', name: 'version', value: question.version }),
    // The number a refusal names the question by.
    question.number > 0 ? h('input', { type: 'hidden', name: 'number', value: question.number }) : null,
  ];
}

/** "Back to the drawing board…", which a first click turns into its confirmation.
 * @param {QuestionView} question the question it answers */
export function drawingBoard(question) {
  /** @type {HTMLElement[]} */
  let shown = [];
  const ask = h(
    'button',
    { class: 'button secondary block', type: 'button', onclick: () => arm() },
    'Back to the drawing board…',
  );
  const form = h('form', { class: 'drawing-board', 'data-method': 'answer' }, fields(question), ask);
  const arm = () => {
    const confirm = h('button', { class: 'button danger block', type: 'submit' }, CONFIRM);
    shown = [confirm, h('p', { class: 'hint' }, HINT)];
    ask.replaceWith(...shown);
    confirm.focus();
  };
  form.addEventListener('keydown', (event) => {
    if (event.key !== 'Escape' || shown.length === 0) return;
    for (const node of shown.slice(1)) node.remove();
    shown[0].replaceWith(ask);
    shown = [];
    ask.focus();
  });
  return form;
}

/** The confirmation, which answers `question`: in the menu, in place of its entries.
 * @param {QuestionView} question */
export function drawingBoardConfirmation(question) {
  return h(
    'form',
    { class: 'drawing-board', 'data-method': 'answer' },
    fields(question),
    h('p', { class: 'hint' }, HINT),
    h('button', { class: 'button danger', type: 'submit' }, CONFIRM),
  );
}

/** What Back to the drawing board sends: the question it answers, and that it is no answer.
 * @param {QuestionView} question */
function fields(question) {
  return [answerFields(question), h('input', { type: 'hidden', name: 'drawing_board', value: 'true' })];
}
