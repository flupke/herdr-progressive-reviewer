// Choice cards (choices.css): the options of a question or of a quiz item, each a card with its
// radio on the first line, named by its text alone. On a question, a recommended choice carries
// the agent's tag and its reason, which describes it, and once the recommendation of a blind
// question shows, the reviewer's first pick keeps its tag.

/** @import { ChoiceView } from "./types.ts" */

import { h } from './dom.js';

/** The choices as cards, under their legend. `name` is the radios' field, which also names
 * each card's text (`<name>-N`); `picking` requires a pick, and `revealed` tags the first pick
 * once the recommendation shows.
 * @param {ChoiceView[]} choices
 * @param {{ name?: string, legend?: string, picking?: boolean, revealed?: boolean }} [options] */
export function choiceCards(choices, { name = 'choice', legend = 'Choices', picking = false, revealed = false } = {}) {
  return h(
    'fieldset',
    { class: 'choices' },
    h('legend', { class: 'eyebrow' }, legend),
    choices.map((choice, index) => {
      const reason = choice.recommendation !== null ? `recommendation-${index + 1}` : null;
      const text = `${name}-${index + 1}`;
      const firstPick = revealed && choice.checked;
      return h(
        'label',
        { class: choice.recommendation !== null ? 'choice recommended' : 'choice' },
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
        h('span', { class: 'choice-text', id: text }, choice.text),
        firstPick || reason
          ? h(
              'span',
              { class: 'choice-tags' },
              firstPick ? h('span', { class: 'tag accent' }, 'Your first pick') : null,
              reason ? h('span', { class: 'tag agent' }, '◆ Agent recommends') : null,
            )
          : null,
        reason ? h('span', { class: 'choice-reason', id: reason }, choice.recommendation) : null,
      );
    }),
  );
}
