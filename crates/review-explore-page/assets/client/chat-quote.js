// "Add to chat" (docs/design/explore-page/README.md, "Agent chat", selection → quote): when the
// reviewer selects a passage of what the page gives to read (the question, its context, Door and
// Blast radius, the citations, the design, the conclusion), one option shows above it, which
// opens the chat with the passage quoted in its composer. A selection in the panel, a form, the
// masthead or the chat itself offers nothing.

import { requestChat } from './chat.js';
import { h } from './dom.js';

/** The longest quote, in characters: a passage, not a page. */
const LONGEST = 1000;

/** Where a selection offers nothing. */
const NOT_READING = '.panel, form, textarea, input, button, .masthead, .chat';

export class QuotePicker {
  /** @param {HTMLElement} main the page's reading area */
  constructor(main) {
    this.main = main;
    /** Whether the round has a conversation the reviewer can write to. */
    this.writable = false;
    this.button = h(
      'button',
      {
        class: 'chat-pick',
        type: 'button',
        hidden: true,
        // A press keeps the selection, which the click then quotes.
        onpointerdown: (/** @type {Event} */ event) => event.preventDefault(),
        onclick: () => this.pick(),
      },
      'Add to chat',
    );
    document.body.append(this.button);
    /** The passage the button quotes. */
    this.passage = '';
    document.addEventListener('selectionchange', () => this.follow());
  }

  /** @param {boolean} writable whether the round has a conversation the reviewer can write to */
  update(writable) {
    this.writable = writable;
    if (!writable) this.hide();
  }

  /** Shows the button above the reviewer's selection, when it is a passage of the reading. */
  follow() {
    const selection = document.getSelection();
    const passage = selection ? quotable(selection, this.main) : null;
    if (!this.writable || !selection || !passage) {
      this.hide();
      return;
    }
    this.passage = passage;
    const box = selection.getRangeAt(0).getBoundingClientRect();
    this.button.hidden = false;
    const width = this.button.offsetWidth;
    const left = Math.min(Math.max(8, box.left + box.width / 2 - width / 2), window.innerWidth - width - 8);
    const above = box.top - this.button.offsetHeight - 8;
    const top = above >= 8 ? above : box.bottom + 8;
    this.button.style.left = `${left + window.scrollX}px`;
    this.button.style.top = `${top + window.scrollY}px`;
  }

  pick() {
    const passage = this.passage;
    this.hide();
    document.getSelection()?.removeAllRanges();
    requestChat(passage);
  }

  hide() {
    this.button.hidden = true;
    this.passage = '';
  }
}

/**
 * The text of `selection`, with its white space folded, when it is a passage of what `main`
 * gives to read; `null` otherwise.
 * @param {Selection} selection
 * @param {HTMLElement} main
 */
function quotable(selection, main) {
  if (selection.isCollapsed || selection.rangeCount === 0) return null;
  const inReading = (/** @type {Node | null} */ node) => {
    const element = node instanceof Element ? node : node?.parentElement;
    return element !== null && element !== undefined && main.contains(element) && !element.closest(NOT_READING);
  };
  if (!inReading(selection.anchorNode) || !inReading(selection.focusNode)) return null;
  const text = selection.toString().replace(/\s+/g, ' ').trim();
  if (!text) return null;
  return text.length > LONGEST ? `${text.slice(0, LONGEST - 1)}…` : text;
}
