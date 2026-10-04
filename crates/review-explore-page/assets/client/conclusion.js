// The agent's conclusion: its summary, the list to be implemented in the panel with what became
// of its latest implementation request and the reply to the conclusion, the future work, and
// the results of its quiz.
//
// The section stays while the page shows the same conclusion, and each part is rebuilt only when
// its own data changes: a request that goes from being sent to received changes its card and
// leaves a reply being typed as it is.

/** @import { ConclusionView } from "./types.ts" */

import { h, keyOf, markdown, Region } from './dom.js';
import { keepDraft } from './drafts.js';
import { quizResults } from './quiz.js';
import { statusCard } from './status.js';

export class ConclusionScreen {
  constructor() {
    this.element = h(
      'section',
      { class: 'conclusion desk wide', 'aria-labelledby': 'conclusion-title' },
      h('h2', { id: 'conclusion-title' }, 'Conclusion'),
    );
    this.summary = new Region(this.element, 'summary');
    this.panel = new Region(this.element, 'panel');
    this.future = new Region(this.element, 'future-work');
    this.results = new Region(this.element, 'quiz-results');
  }

  /** @param {ConclusionView} conclusion */
  update(conclusion) {
    this.summary.show(conclusion.summary_html, () => markdown(conclusion.summary_html));
    this.panel.component(conclusion.request, () => new Panel()).update(conclusion);
    const future = conclusion.future_work_html;
    this.future.show(future, () =>
      future === null
        ? null
        : h(
            'section',
            { 'aria-labelledby': 'future-work-title' },
            h('h3', { id: 'future-work-title' }, 'Future work'),
            markdown(future),
          ),
    );
    const quiz = conclusion.quiz;
    this.results.show(keyOf(quiz), () => (quiz ? quizResults(quiz) : null));
  }
}

/** The list to be implemented, with the reviewer's actions on the conclusion. */
class Panel {
  constructor() {
    this.element = h(
      'section',
      { class: 'to-be-implemented panel', 'aria-labelledby': 'to-be-implemented-title' },
      h('h3', { id: 'to-be-implemented-title' }, 'To be implemented'),
    );
    this.card = new Region(this.element, 'implementation');
    this.list = new Region(this.element, 'list');
    this.reply = new Region(this.element, 'reply');
  }

  /** @param {ConclusionView} conclusion */
  update(conclusion) {
    const card = conclusion.implementation_card;
    this.card.show(keyOf(card), () => (card ? statusCard(card) : null));
    const { offers_implement, implementation, offers_actions, draft, draft_html, request } = conclusion;
    this.list.show(keyOf({ offers_implement, implementation, offers_actions, draft, draft_html }), () =>
      list(conclusion),
    );
    this.reply.show(keyOf([request, offers_actions]), () => (offers_actions ? replyForm(request) : null));
  }
}

/** The list as raw text the reviewer edits, then sends with Implement; or the list a request
 * sends, once a request is on its way.
 * @param {ConclusionView} conclusion */
function list(conclusion) {
  const implementation = conclusion.implementation;
  if (!conclusion.offers_implement) {
    if (implementation) return markdown(implementation.text_html);
    return conclusion.offers_actions ? null : markdown(conclusion.draft_html);
  }
  const kind = implementation?.state.kind;
  const replacing = implementation !== null && (kind === 'paused' || kind === 'unknown');
  const replaces = implementation?.delivery ?? null;
  return [
    replacing ? markdown(implementation.text_html) : null,
    h(
      'form',
      { class: 'implement', 'data-method': 'implement' },
      h('input', { type: 'hidden', name: 'conclusion', value: conclusion.request }),
      replaces !== null ? h('input', { type: 'hidden', name: 'replaces', value: replaces }) : null,
      keepDraft(
        h('textarea', { class: 'tasks', name: 'text', rows: 6, 'aria-labelledby': 'to-be-implemented-title' }),
        `implement:${conclusion.request}:${replaces ?? ''}`,
        conclusion.draft,
      ),
      h('p', { class: 'hint' }, 'Implement asks the agent to implement this list, and nothing else.'),
      replacing
        ? h('button', { class: 'button secondary block', type: 'submit' }, 'Send a new request')
        : h('button', { class: 'button primary block', type: 'submit' }, 'Implement'),
    ),
  ].filter((node) => node !== null);
}

/** A reply to the conclusion, as Reply in the reviewer's Explore tab: free text, which the agent
 * takes up in its next turn.
 * @param {string} conclusion */
function replyForm(conclusion) {
  return h(
    'form',
    { class: 'reply', 'data-method': 'reply' },
    h('input', { type: 'hidden', name: 'conclusion', value: conclusion }),
    h('label', { class: 'comment-label', for: 'reply' }, 'Reply to the conclusion'),
    keepDraft(
      h('textarea', { class: 'comment', id: 'reply', name: 'text', rows: 3, required: true }),
      `reply:${conclusion}`,
      '',
    ),
    h('p', { class: 'hint' }, 'The agent takes your reply up in its next turn.'),
    h('button', { class: 'button secondary', type: 'submit' }, 'Send the reply'),
  );
}
