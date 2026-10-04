// The answer the agent's turn carries, while the agent works on the turn or the turn did not go
// through (docs/design/explore-page/README.md, "3. Waiting"; design review, findings 6 and 13):
// on the desk, the turn's status card, then the question the answer answers, in muted text, with
// its Door and Blast radius and its citations folded; in the panel, the answer that was sent,
// what it marked, the card's one action (Stop waiting, or Retry) and Cancel this answer.
//
// The parts are rebuilt only when their own data changes, so the time since the turn went out
// keeps counting, and the reader's folds stay open, while the round moves on in the pane.

/** @import { SentView, LatestAnswer } from "./types.ts" */

import { answerCard, markedLine } from './answer-card.js';
import { citationsSection } from './citations.js';
import { h, keyOf, Region } from './dom.js';
import { questionReading } from './question.js';
import { panelActions, panelCard } from './status.js';
import { cancelAnswer } from './turn.js';

export class SentScreen {
  constructor() {
    this.element = h('div', { class: 'sent desk' });
    // On a narrow window the desk shows its parts in this order: the panel comes right after the
    // card, so that what was sent and the one action come before the question read again.
    this.card = new Region(this.element, 'card');
    this.panel = new Region(this.element, 'panel');
    this.reading = new Region(this.element, 'reading');
    this.citations = new Region(this.element, 'citations');
  }

  /**
   * @param {SentView} sent
   * @param {LatestAnswer | null} cancellable the reviewer's latest answer, when the page offers
   *   to cancel it
   */
  update(sent, cancellable) {
    this.card.show(keyOf(sent.card), () => panelCard(sent.card));
    const question = sent.question;
    this.reading.show(keyOf(question && [question.id, question.version, question.number]), () => {
      if (!question) return null;
      const label = `question-${question.number}-label`;
      return h(
        'section',
        { class: 'answered-question', 'aria-labelledby': label },
        questionReading(
          { ...question, label, name: `Question ${question.number} · answered` },
          question.recommendation !== 'shown' ? h('span', { class: 'chip agent' }, 'Blind pick') : null,
        ),
      );
    });
    this.panel.show(keyOf([sent, cancellable]), () => panel(sent, cancellable));
    this.citations.show(keyOf(question?.citations ?? null), () =>
      question ? citationsSection(question.citations, question.number, { folded: true }) : null,
    );
  }
}

/** What the reviewer sent, what it marked, then the card's action and Cancel this answer.
 * @param {SentView} sent
 * @param {LatestAnswer | null} cancellable */
function panel(sent, cancellable) {
  const title = sent.question
    ? sent.number !== null
      ? `Your answer to question ${sent.number}`
      : 'Your answer'
    : 'Your reply to the conclusion';
  return h(
    'section',
    { class: 'sent-answer panel', 'aria-labelledby': 'sent-answer-title' },
    h('p', { class: 'eyebrow', id: 'sent-answer-title' }, title),
    answerCard(sent.answer),
    sent.marked ? markedLine(sent.marked) : null,
    panelActions(sent.card),
    cancellable ? cancelAnswer(cancellable) : null,
  );
}
