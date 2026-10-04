// An earlier question: a question the reviewer answered before in the round, opened from its done
// step on the round rail (`#question-N`, route.js) or reached with a swipe on a phone. On the
// desk, the question as it was asked, with its Context, Door and Blast radius rows and its
// citations; in the panel, what the reviewer answered, what the answer marked and what the agent
// recorded, then the way back to the step the round stands at. Everything is read only: the
// screen holds no form, and its data carries no identity an action could post with, so nothing
// on it can change the round.

/** @import { EarlierQuestionView } from "./types.ts" */
/** @import { Current } from "./design.js" */

import { answerCard, markedLine } from './answer-card.js';
import { citationsSection } from './citations.js';
import { goTo } from './design.js';
import { h } from './dom.js';
import { questionReading } from './question.js';
import { followUpsLine, recorded } from './turn.js';

/**
 * The screen of an earlier question.
 * @param {EarlierQuestionView} question
 * @param {Current | null} current the step the round stands at, which the way back goes to
 */
export function earlierScreen(question, current) {
  const label = `earlier-${question.number}-label`;
  // On a phone, the panel follows the head: the answer comes before the rest of the reading.
  const [head, ...reading] = questionReading(
    { ...question, label, name: `Question ${question.number} · answered` },
    null,
  );
  return h(
    'section',
    { class: 'earlier-question desk', 'aria-labelledby': label },
    head,
    panel(question, current),
    reading,
    citationsSection(question.citations, question.number),
  );
}

/** What the reviewer answered and what it marked, what the agent recorded of it, and the way
 * back to the round.
 * @param {EarlierQuestionView} question
 * @param {Current | null} current */
function panel(question, current) {
  const title = `earlier-${question.number}-answer`;
  return h(
    'section',
    { class: 'earlier-panel panel', 'aria-labelledby': title },
    h('p', { class: 'eyebrow', id: title }, `Your answer to question ${question.number}`),
    question.answer ? answerCard(question.answer) : h('p', { class: 'hint' }, 'The round left this question unanswered.'),
    question.marks.map(markedLine),
    record(question),
    current ? goTo(current) : null,
  );
}

/** What the agent recorded of the answer and replied, then the follow-ups it noted; nothing
 * when it said nothing.
 * @param {EarlierQuestionView} question */
function record(question) {
  const response = question.recorded;
  const said = response.interpretations.some((interpretation) => interpretation.recap_html) || response.reply_html;
  const followUps = followUpsLine(response, 'earlier-follow-ups');
  if (!said && !followUps) return null;
  return h(
    'div',
    { class: 'earlier-record' },
    said ? recorded(response, `earlier-${question.number}-record`) : null,
    followUps,
  );
}
