// Choice cards (choices.css): the options of a question or of a quiz item, each a card with its
// radio on the first line, named by its text alone. On a question, a recommended choice carries
// the agent's tag and its reason, which describes it, and once the recommendation of a blind
// question shows, the reviewer's first pick keeps its tag.

/** @import { ChoiceView } from "./types.ts" */

import { codeSpans, h } from './dom.js';

/** Options with nothing of the agent's on them, such as a quiz item's answers, as choice cards
 * take them: none recommended, none picked yet.
 * @param {{ id: string, text: string }[]} options
 * @returns {ChoiceView[]} */
export function plainChoices(options) {
  return options.map(({ id, text }) => ({ id, text, recommendation: null, checked: false }));
}

/** The choices as cards, under their legend. `name` is the radios' field, which also names
 * each card's text (`<name>-N`); `picking` requires a pick, and `revealed` tags the first pick
 * once the recommendation shows; `answered` shows the choices of a question the reviewer
 * answered, read only, the kept one selected.
 * @param {ChoiceView[]} choices
 * @param {{ name?: string, legend?: string, picking?: boolean, revealed?: boolean, answered?: boolean }} [options] */
export function choiceCards(
  choices,
  { name = 'choice', legend = 'Choices', picking = false, revealed = false, answered = false } = {},
) {
  return h(
    'fieldset',
    { class: answered ? 'choices answered' : 'choices', disabled: answered },
    // Read again under the panel's own eyebrow, an answered question's legend is for readers only.
    h('legend', { class: answered ? 'sr-only' : 'eyebrow' }, legend),
    choices.map((choice, index) => {
      const { recommendation } = choice;
      const reason = recommendation !== null ? `recommendation-${index + 1}` : null;
      const text = `${name}-${index + 1}`;
      const firstPick = revealed && choice.checked;
      return h(
        'label',
        { class: reason ? 'choice recommended' : 'choice' },
        h('input', {
          type: 'radio',
          name,
          value: choice.id,
          checked: choice.checked,
          required: picking,
          // The choice is named by its text alone: its tags and the agent's reason describe it.
          'aria-labelledby': text,
          'aria-describedby': reason,
        }),
        h('span', { class: 'choice-text', id: text }, codeSpans(choice.text)),
        firstPick || reason
          ? h(
              'span',
              { class: 'choice-tags' },
              firstPick ? h('span', { class: 'tag accent' }, 'Your first pick') : null,
              reason ? h('span', { class: 'tag agent' }, '◆ Agent recommends') : null,
            )
          : null,
        recommendation !== null ? h('span', { class: 'choice-reason', id: reason }, codeSpans(recommendation)) : null,
      );
    }),
  );
}
