// The end of the round (docs/design/explore-page/README.md, "5. Conclusion"): the conclusion to
// read on the left, with the reviewer's decisions, and on the right the panel with the list to
// be implemented, whole, and one primary action for each state of the implementation request.
//
// The section stays while the page shows the same conclusion, and each part is rebuilt only when
// its own data changes: a request that goes from being sent to received changes its card and
// leaves a reply being typed as it is.

/** @import { ConclusionView, DecisionView, ImplementationView, QuizView } from "./types.ts" */
/** @import { Turn } from "./turn.js" */

import { h, keyOf, markdown, Region } from './dom.js';
import { keepDraft } from './drafts.js';
import { openQuizResults, quizResults } from './quiz.js';
import { disclosure } from './disclosure.js';
import { resetConfirmation } from './masthead.js';
import { panelActions, panelCard } from './status.js';
import { turnStrip } from './turn.js';

/** The words of each tag of a decision, and its colour. */
const TAGS = {
  changed_after_first_pick: { words: 'changed after your first pick', tone: 'changed' },
  as_recommended: { words: 'as recommended', tone: 'recommended' },
};

/** A request's list shows this many items before "… N more". */
const SHOWN_ITEMS = 3;

export class ConclusionScreen {
  constructor() {
    this.element = h('section', { class: 'conclusion desk wide', 'aria-labelledby': 'conclusion-title' });
    // The previous turn, above the conclusion it led to.
    this.turn = new Region(this.element, 'turn');
    this.element.append(
      h('p', { class: 'eyebrow' }, 'The round is over'),
      h('h2', { id: 'conclusion-title' }, 'Conclusion'),
    );
    this.lead = new Region(this.element, 'lead');
    this.decisions = new Region(this.element, 'decisions');
    this.summary = new Region(this.element, 'summary');
    this.future = new Region(this.element, 'future-work');
    this.results = new Region(this.element, 'quiz-results');
    this.panel = new Region(this.element, 'panel');
  }

  /**
   * @param {ConclusionView} conclusion
   * @param {string | null} reset the round Reset closes, when the page offers it
   * @param {Turn} turn the turn that led to the conclusion
   */
  update(conclusion, reset, turn) {
    this.turn.show(keyOf(turn), () => turnStrip(turn));
    const lead = conclusion.lead_html;
    this.lead.show(keyOf(lead), () => (lead === null ? null : markdown(lead, 'lead')));
    const decisions = conclusion.decisions;
    this.decisions.show(keyOf(decisions), () => (decisions.length > 0 ? decisionList(decisions) : null));
    this.summary.show(conclusion.summary_html, () => markdown(conclusion.summary_html, 'summary'));
    const future = conclusion.future_work_html;
    this.future.show(keyOf(future), () =>
      future === null
        ? null
        : h(
            'section',
            { class: 'future-work', 'aria-labelledby': 'future-work-title' },
            h('h3', { class: 'eyebrow', id: 'future-work-title' }, 'Future work'),
            markdown(future),
          ),
    );
    const quiz = conclusion.quiz;
    this.results.show(keyOf(quiz), () => {
      if (!quiz) return null;
      const results = quizResults(quiz);
      results.id = 'quiz-results';
      return results;
    });
    this.panel.component(conclusion.request, () => new Panel()).update(conclusion, reset);
  }
}

/**
 * "Your decisions": each question the reviewer answered, with the choice kept and its tags, or
 * the comment of an answer that kept no choice.
 * @param {DecisionView[]} decisions
 */
function decisionList(decisions) {
  return h(
    'section',
    { class: 'decisions', 'aria-labelledby': 'decisions-title' },
    h('h3', { class: 'eyebrow', id: 'decisions-title' }, 'Your decisions'),
    h(
      'ol',
      { class: 'decision-list' },
      decisions.map((decision) =>
        h(
          'li',
          { class: 'decision' },
          h('span', { class: 'decision-number' }, `Q${decision.number}`),
          h(
            'div',
            { class: 'decision-body' },
            h('p', { class: 'decision-question' }, decision.question),
            decision.choice !== null
              ? h(
                  'p',
                  { class: 'decision-kept' },
                  h('strong', {}, decision.choice),
                  decision.tags.map((tag) => [' ', h('span', { class: `tag ${TAGS[tag].tone}` }, TAGS[tag].words)]),
                )
              : null,
            decision.comment
              ? h(
                  'p',
                  { class: decision.choice === null ? 'decision-kept comment-only' : 'decision-comment' },
                  decision.comment,
                )
              : null,
          ),
        ),
      ),
    ),
  );
}

/** The panel: the quiz's score, then the list to be implemented and what became of its latest
 * request, with the one primary action of each state, then the reply under its disclosure. */
class Panel {
  constructor() {
    this.element = h('section', { class: 'to-be-implemented panel', 'aria-label': 'To be implemented' });
    this.banner = new Region(this.element, 'quiz-banner');
    this.card = new Region(this.element, 'implementation');
    this.list = new Region(this.element, 'list');
    this.reply = new Region(this.element, 'reply');
    /** The button that opens the reply, whose words follow the request.
     * @type {HTMLButtonElement | null} */
    this.replyToggle = null;
  }

  /**
   * @param {ConclusionView} conclusion
   * @param {string | null} reset
   */
  update(conclusion, reset) {
    const quiz = conclusion.quiz;
    this.banner.show(keyOf(quiz), () => (quiz ? quizBanner(quiz) : null));
    const card = conclusion.implementation_card;
    this.card.show(keyOf(card), () => (card ? panelCard(card) : null));
    const { list: shown, implementation, draft, request, reply_label: reply } = conclusion;
    this.list.show(keyOf({ shown, implementation, card, draft, request, reset }), () => list(conclusion, reset));
    // The fold keeps a reply being typed, open, while the request goes out: only its words
    // follow the request.
    this.reply.show(keyOf([request, reply !== null]), () => {
      if (reply === null) return null;
      const fold = replyFold(request, reply);
      this.replyToggle = fold.toggle;
      return fold.element;
    });
    if (reply !== null && this.replyToggle) this.replyToggle.textContent = reply;
  }
}

/**
 * The quiz's score at the top of the panel, which matters right at the Implement decision: warn
 * below two-thirds, good from there, with a link to the results.
 * @param {QuizView} quiz
 */
function quizBanner(quiz) {
  const items = quiz.items.length;
  const missed = quiz.picked - quiz.correct_picks;
  const unanswered = items - quiz.picked;
  const questions = (/** @type {number} */ count) => `${count} ${count === 1 ? 'question' : 'questions'}`;
  const words = [
    missed > 0 ? `You missed ${questions(missed)}.` : null,
    unanswered > 0 ? `You ${quiz.skipped ? 'skipped' : 'did not answer'} ${questions(unanswered)}.` : null,
  ].filter((part) => part !== null);
  const tone = quiz.correct_picks * 3 >= items * 2 ? 'good' : 'warn';
  return h(
    'p',
    { class: `quiz-banner ${tone}` },
    h('strong', {}, `Quiz ${quiz.correct_picks} of ${items}`),
    ' ',
    words.length > 0 ? words.join(' ') : 'Every answer was correct.',
    ' ',
    h(
      'a',
      {
        href: '#quiz-results',
        onclick: openQuizResults,
      },
      'See the answers',
    ),
  );
}

/**
 * The list: the text the reviewer edits and sends with Implement, or the list read only, then
 * the actions of the request's state: the card's, "Edit before sending", "Start a new round…".
 * @param {ConclusionView} conclusion
 * @param {string | null} reset the round Reset closes, when the page offers it
 * @returns {HTMLElement | HTMLElement[]}
 */
function list(conclusion, reset) {
  const { list: shown, implementation, implementation_card: card } = conclusion;
  switch (shown.kind) {
    case 'editable':
      return implementForm(conclusion, null, 'primary');
    case 'draft':
      return [eyebrow('To be implemented', shown.items), listBlock(markdown(shown.html), false)];
    case 'request':
      if (!implementation) return [];
      return [
        savedList(implementation, shown.label, shown.settled),
        card ? panelActions(card) : [],
        shown.edit ? [disclosure('Edit before sending', implementForm(conclusion, 'Send a new request', 'secondary')).element] : [],
        shown.new_round && reset ? [newRound(reset)] : [],
      ].flat();
  }
}

/**
 * "Start a new round…", once the agent received the request: Reset of the round, as the
 * masthead's menu sends it, worded as starting the next one, behind its confirmation.
 * @param {string} round the round it closes
 */
function newRound(round) {
  const confirmation = resetConfirmation(round, {
    hint: 'A new round closes this one for good: it is no longer shown, and its agent can no longer post to it. Its records stay saved.',
    label: 'Confirm: start a new round',
  });
  return disclosure('Start a new round…', confirmation, { button: 'button secondary block' }).element;
}

/**
 * The list a request sends, read only, under its eyebrow: its first items, and the others behind
 * "… N more".
 * @param {ImplementationView} implementation
 * @param {string} label
 * @param {boolean} settled whether the request is out of the reviewer's hands, which mutes it
 * @returns {HTMLElement[]}
 */
function savedList(implementation, label, settled) {
  const text = listBlock(markdown(implementation.text_html), settled);
  const hidden = [...text.querySelectorAll('li')].filter((item) => item.parentElement?.parentElement === text).slice(SHOWN_ITEMS);
  for (const item of hidden) item.hidden = true;
  if (hidden.length === 0) return [eyebrow(label, implementation.items), text];
  const more = h(
    'button',
    {
      class: 'more',
      type: 'button',
      onclick: () => {
        for (const item of hidden) item.hidden = false;
        more.remove();
      },
    },
    `… ${hidden.length} more`,
  );
  return [eyebrow(label, implementation.items), text, more];
}

/**
 * The form that sends the list the reviewer edits: as the conclusion's first request, in place
 * of one the agent did not receive, or, under "Edit before sending", in place of a saved one.
 * `label` names its button, or `null` for "Implement N items" under the eyebrow "To be
 * implemented · N items", which count the items as the reviewer types.
 * @param {ConclusionView} conclusion
 * @param {string | null} label
 * @param {'primary' | 'secondary'} tier
 */
function implementForm(conclusion, label, tier) {
  const replaces = conclusion.implementation?.delivery ?? null;
  const box = keepDraft(
    h('textarea', { class: 'tasks', name: 'text', 'aria-label': 'To be implemented' }),
    `implement:${conclusion.request}:${replaces ?? ''}`,
    conclusion.draft,
  );
  const count = h('span', {});
  const button = h('button', { class: `button ${tier} block`, type: 'submit' }, label ?? '');
  const follow = () => {
    const items = countItems(box.value);
    count.textContent = itemsWords(items);
    // Without `field-sizing`, the box grows by its rows.
    box.rows = box.value.split('\n').length + 1;
    if (label !== null) return;
    button.textContent = `Implement ${itemsWords(items)}`;
    // The tool refuses an empty list, as the pane does: the button waits for an item
    // (actions.js enables the buttons again after each edit).
    button.dataset.blocked = String(items === 0);
  };
  box.addEventListener('input', follow);
  const form = h(
    'form',
    { class: 'implement', 'data-method': 'implement' },
    h('input', { type: 'hidden', name: 'conclusion', value: conclusion.request }),
    replaces !== null ? h('input', { type: 'hidden', name: 'replaces', value: replaces }) : null,
    label === null ? h('p', { class: 'eyebrow' }, 'To be implemented · ', count) : null,
    box,
    h('p', { class: 'hint' }, 'Implement asks the agent to implement this list, and nothing else.'),
    button,
  );
  follow();
  return form;
}

/**
 * The eyebrow of a list, with its count: "Saved request · 10 items".
 * @param {string} label
 * @param {number} items
 */
function eyebrow(label, items) {
  return h('p', { class: 'eyebrow' }, `${label} · ${itemsWords(items)}`);
}

/**
 * A read-only list on the page's own surface, muted when `settled`.
 * @param {HTMLElement} text
 * @param {boolean} settled
 */
function listBlock(text, settled) {
  text.classList.add('saved-list');
  if (settled) text.classList.add('muted');
  return text;
}

/**
 * How many items the list `text` has: its lines that are not blank, as the tool counts them
 * (`PageImplementation::items` in src/round.rs).
 * @param {string} text
 */
function countItems(text) {
  return text.split('\n').filter((line) => line.trim() !== '').length;
}

/** @param {number} items */
function itemsWords(items) {
  return `${items} ${items === 1 ? 'item' : 'items'}`;
}

/**
 * A reply to the conclusion, as Reply in the reviewer's Explore tab: free text, which the agent
 * takes up in its next turn. It waits under a disclosure, so that the panel has one primary
 * action.
 * @param {string} conclusion
 * @param {string} label the words that open it
 * @returns {{ element: HTMLElement, toggle: HTMLButtonElement }}
 */
function replyFold(conclusion, label) {
  const box = keepDraft(
    h('textarea', { class: 'comment', id: 'reply', name: 'text', rows: 3, required: true }),
    `reply:${conclusion}`,
    '',
  );
  const form = h(
    'form',
    { class: 'reply', 'data-method': 'reply' },
    h('input', { type: 'hidden', name: 'conclusion', value: conclusion }),
    h('label', { class: 'comment-label', for: 'reply' }, 'Reply to the conclusion'),
    box,
    h('p', { class: 'hint' }, 'The agent takes your reply up in its next turn.'),
    h('button', { class: 'button secondary', type: 'submit' }, 'Send the reply'),
  );
  // A reply the reviewer had begun shows open again.
  return disclosure(label, form, { open: box.value !== '' });
}
