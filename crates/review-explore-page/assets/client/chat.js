// The chat: the round's conversation with the agent (docs/design/explore-page/README.md, "Agent
// chat"; prototype ChatDrawer.dc.html). It is a review thread attached to the round: a message
// wakes the agent as a thread's comment does, never answers the question, and the agent's reply
// lands here. It opens from the masthead's bubble, from the ⋯ menu, from "Not ready? Reply to
// the agent instead" on the conclusion, and from "Add to chat" on a passage the reviewer selected
// (chat-quote.js), which it quotes. On a desktop it stands at the window's left, under its bubble
// at the left of the masthead: beside the reading column when the window has room for three
// columns, as wide as its widest message or as the reviewer set it (chat-width.js), over the
// left of the page when it has not, and never over the panel (chat.css, layout.css); below the
// desk's two columns it is a bottom sheet over the dimmed page.
//
// The drawer, its composer and its draft stay while the page shows the same round; the messages
// are rebuilt when they change. A reply the reviewer has not seen shows on the bubble and in the
// tab's title until the chat shows it, which marks it read.

/** @import { AskedUnder, ChatMessageView, ConversationView, PageView, Call, Reply } from "./types.ts" */

import { ChatWidth } from './chat-width.js';
import { DESK } from './desk.js';
import { h, keyOf, markdown, Region } from './dom.js';
import { keep, keepDraft, kept } from './drafts.js';
import { clockTime, statusCard } from './status.js';

/** The event that asks the chat to open, with an optional quote (`requestChat`). */
const OPEN = 'explore:chat';

/** The page's token `name` (tokens.css, layout.css) as a number of pixels or milliseconds.
 * @param {string} name */
function token(name) {
  return parseFloat(getComputedStyle(document.documentElement).getPropertyValue(name)) || 0;
}

/** Whether the browser lets the chat rise with the masthead by itself (chat.css). */
const RISES = CSS.supports('animation-timeline: scroll()');

/** How far a drag of the phone sheet's grabber goes before it changes the sheet. */
const DRAG = 60;

/**
 * Opens the chat, with `quote` as its composer's quote when given.
 * @param {string | null} [quote]
 */
export function requestChat(quote = null) {
  document.dispatchEvent(new CustomEvent(OPEN, { detail: { quote } }));
}

/** What the page shows beside the chat, which a message is written under.
 * @typedef {{ askedUnder: AskedUnder | null, question: boolean }} Place */

export class Chat {
  /**
   * @param {HTMLElement} slot the masthead's place for the bubble
   * @param {(call: Call) => Promise<Reply>} call sends a request on the page's socket
   * @param {() => void} redraw draws the page again, after the chat opened or closed
   */
  constructor(slot, call, redraw) {
    this.call = call;
    this.redraw = redraw;
    this.open = false;
    /** @type {ConversationView | null} */
    this.conversation = null;
    /** The position the page asked to mark read through, so that it asks once. */
    this.readAsked = -1;
    this.bubble = h(
      'button',
      {
        class: 'chat-bubble',
        type: 'button',
        'aria-label': 'Talk to the agent',
        'aria-expanded': 'false',
        'aria-controls': 'chat',
        hidden: true,
        onclick: () => (this.open ? this.close(true) : this.show()),
      },
      h('span', { class: 'bubble-shape', 'aria-hidden': 'true' }),
    );
    this.badge = h('span', { class: 'chat-badge', hidden: true });
    this.bubble.append(this.badge);
    slot.append(this.bubble);
    this.scrim = h('div', { class: 'chat-scrim', hidden: true, onclick: () => this.close(false) });
    this.element = h('aside', { class: 'chat', id: 'chat', 'aria-label': 'Conversation with the agent', hidden: true });
    const grabber = h('div', { class: 'chat-grabber', 'aria-hidden': 'true' });
    dragSheet(grabber, this);
    this.meta = h('span', { class: 'chat-meta' });
    this.element.append(
      grabber,
      h(
        'header',
        { class: 'chat-head' },
        h('h2', { class: 'chat-title' }, 'Agent'),
        this.meta,
        h('button', { class: 'chat-close', type: 'button', 'aria-label': 'Close the conversation', onclick: () => this.close(true) }, '×'),
      ),
    );
    this.log = h('div', { class: 'chat-log', role: 'log', 'aria-label': 'Messages' });
    this.element.append(this.log);
    this.width = new ChatWidth(this.element, this.log);
    this.messages = new Region(this.log, 'messages');
    this.pending = new Region(this.log, 'pending');
    this.card = new Region(this.log, 'card');
    this.composer = new Region(this.element, 'composer');
    /** @type {Composer | null} */
    this.form = null;
    document.body.append(this.scrim, this.element);
    document.addEventListener(OPEN, (event) => {
      const quote = /** @type {CustomEvent<{ quote: string | null }>} */ (event).detail.quote;
      this.show(quote);
    });
    // On the window, which hears a key after every listener of the document: an Escape that
    // closed something else (a popover of the masthead, the meter's pinned window, a diagram
    // opened large) leaves the chat open.
    addEventListener('keydown', (event) => {
      if (event.key !== 'Escape' || !this.open || event.defaultPrevented) return;
      if (event.target instanceof Element && event.target.closest('dialog')) return;
      this.close(true);
    });
    // A reply that came while the tab was hidden is seen once the tab shows again.
    document.addEventListener('visibilitychange', () => {
      this.markRead();
      this.redraw();
    });
    this.timer = 0;
    if (!RISES) {
      addEventListener('scroll', () => this.rise(), { passive: true });
      addEventListener('resize', () => this.rise());
    }
  }

  /** Where the browser has no scroll timeline: on a desktop, the chat rises with the masthead as
   * the page scrolls, to where the panel is held, as `chat-rise` in chat.css does with one:
   * change both together. */
  rise() {
    if (this.element.hidden) return;
    this.element.style.top = '';
    if (!DESK.matches) return;
    const top = parseFloat(getComputedStyle(this.element).top);
    const held = token('--sticky-top');
    this.element.style.top = `${Math.max(held, top - scrollY)}px`;
  }

  /**
   * @param {PageView} view
   * @param {Place} place
   */
  update(view, place) {
    const conversation = view.conversation;
    this.conversation = conversation;
    this.bubble.hidden = conversation === null;
    if (!conversation) {
      if (this.open) this.close(false);
      this.messages.clear();
      this.pending.clear();
      this.card.clear();
      this.composer.clear();
      this.form = null;
      return;
    }
    const count = conversation.messages.length;
    this.meta.textContent = `this round · ${count} ${count === 1 ? 'message' : 'messages'}`;
    const rebuilt = this.messages.show(keyOf(conversation.messages), () =>
      conversation.messages.length > 0 ? conversation.messages.map(message) : h('p', { class: 'chat-empty hint' }, 'Ask, challenge, or add context. The agent replies here; a message answers nothing.'),
    );
    const waiting = waitingSince(conversation.messages);
    this.pending.show(keyOf(waiting), () => (waiting === null ? null : pendingRow(waiting)));
    this.tick(waiting !== null);
    this.card.show(keyOf(conversation.card), () => (conversation.card ? statusCard(conversation.card) : null));
    this.form = this.composer.component(`${conversation.round}:${conversation.writable}`, () => new Composer(conversation));
    this.form.place(place);
    if (rebuilt && this.open) {
      this.width.fit();
      this.scrollToEnd();
    }
    this.markRead();
    this.showUnread();
  }

  /** How many unread replies the bubble and the tab's title show: none while the chat shows
   * them, in a tab the reviewer sees. */
  unread() {
    if (!this.conversation || this.seen()) return 0;
    return this.conversation.unread;
  }

  /** Whether the reviewer sees the chat: open, in a tab in view. */
  seen() {
    return this.open && document.visibilityState === 'visible';
  }

  /** @param {string | null} [quote] */
  show(quote = null) {
    if (!this.conversation) return;
    const opened = !this.open;
    this.open = true;
    this.element.hidden = false;
    if (!RISES) this.rise();
    if (opened) this.width.fit();
    this.scrim.hidden = false;
    this.bubble.setAttribute('aria-expanded', 'true');
    // The next frame, so that the drawer slides in from where it was drawn closed.
    requestAnimationFrame(() => this.element.classList.add('open'));
    if (quote !== null) this.form?.quote(quote);
    if (opened) this.scrollToEnd();
    this.form?.focus();
    this.markRead();
    this.redraw();
  }

  /** @param {boolean} refocus whether to give the focus back to the bubble */
  close(refocus) {
    if (!this.open) return;
    this.open = false;
    this.element.classList.remove('open', 'full');
    // Hidden once it faded out (chat.css), unless it opened again meanwhile.
    setTimeout(() => {
      if (!this.open) this.element.hidden = true;
    }, token('--drawer-out'));
    this.scrim.hidden = true;
    this.bubble.setAttribute('aria-expanded', 'false');
    if (refocus) this.bubble.focus();
    this.redraw();
  }

  /** Marks the replies the chat shows read, once for each position. */
  markRead() {
    const conversation = this.conversation;
    if (!this.seen() || !conversation || conversation.unread === 0) return;
    if (conversation.read_through === this.readAsked) return;
    this.readAsked = conversation.read_through;
    this.call({ method: 'read-messages', params: { round: conversation.round, through: conversation.read_through } }).catch(() => {
      // The socket closed: the next view still counts the reply unread, and asks again.
      this.readAsked = -1;
    });
  }

  showUnread() {
    const unread = this.unread();
    this.badge.hidden = unread === 0;
    this.badge.textContent = unread === 0 ? '' : String(unread);
    if (unread === 0) this.bubble.removeAttribute('aria-description');
    else this.bubble.setAttribute('aria-description', `${unread} unread ${unread === 1 ? 'reply' : 'replies'}`);
    this.bubble.classList.toggle('open', this.open);
  }

  scrollToEnd() {
    this.log.scrollTop = this.log.scrollHeight;
  }

  /** Counts the time of the pending reply while one is pending.
   * @param {boolean} pending */
  tick(pending) {
    if (pending && !this.timer) {
      this.timer = window.setInterval(() => {
        for (const time of this.log.querySelectorAll('[data-since]')) {
          time.textContent = elapsed(Number(/** @type {HTMLElement} */ (time).dataset.since));
        }
      }, 1000);
    } else if (!pending && this.timer) {
      clearInterval(this.timer);
      this.timer = 0;
    }
  }
}

/** The composer: the quote, the text box, Send. A form of actions.js (`send-message`). */
class Composer {
  /** @param {ConversationView} conversation */
  constructor(conversation) {
    const round = conversation.round;
    this.quoteKey = `chat-quote:${round}`;
    this.id = h('input', { type: 'hidden', name: 'id', value: messageId() });
    this.stage = h('input', { type: 'hidden', name: 'stage', value: '' });
    this.question = h('input', { type: 'hidden', name: 'question', value: '' });
    this.version = h('input', { type: 'hidden', name: 'version', value: '' });
    this.conclusion = h('input', { type: 'hidden', name: 'conclusion', value: '' });
    this.quoted = h('input', { type: 'hidden', name: 'quote', value: '' });
    this.quoteText = h('span', { class: 'chat-quote-text' });
    this.quoteChip = h(
      'div',
      { class: 'chat-quote', hidden: true },
      this.quoteText,
      h('button', { class: 'chat-quote-remove', type: 'button', 'aria-label': 'Remove the quote', onclick: () => this.quote(null) }, '×'),
    );
    this.box = keepDraft(
      h('textarea', {
        class: 'chat-text',
        name: 'text',
        'aria-label': 'Message to the agent',
        placeholder: 'Ask, challenge, or add context…',
      }),
      `chat:${round}`,
      '',
    );
    this.box.addEventListener('keydown', (event) => {
      if (event.key === 'Enter' && (event.metaKey || event.ctrlKey)) {
        event.preventDefault();
        this.element.requestSubmit();
      }
    });
    this.hint = h('span', { class: 'hint' });
    this.element = h(
      'form',
      { class: 'chat-composer', 'data-method': 'send-message', 'data-requires': 'text', hidden: !conversation.writable },
      h('input', { type: 'hidden', name: 'round', value: round }),
      this.id,
      this.stage,
      this.question,
      this.version,
      this.conclusion,
      this.quoted,
      this.quoteChip,
      this.box,
      h('div', { class: 'chat-send' }, h('button', { class: 'button outline', type: 'submit' }, 'Send'), this.hint),
    );
    // A message sent clears the composer for the next one, under a new identity.
    this.element.addEventListener('sent', () => {
      this.box.value = '';
      this.box.dispatchEvent(new Event('input', { bubbles: true }));
      this.quote(null);
      this.id.value = messageId();
    });
    this.quote(kept(this.quoteKey));
  }

  /** Where the page shows the chat, which the message names.
   * @param {Place} place */
  place({ askedUnder, question }) {
    this.stage.value = askedUnder?.stage ?? '';
    this.question.value = askedUnder?.stage === 'question' ? askedUnder.question : '';
    this.version.value = askedUnder?.stage === 'question' ? String(askedUnder.version) : '';
    this.conclusion.value = askedUnder?.stage === 'conclusion' ? askedUnder.conclusion : '';
    const keys = /Mac|iPhone|iPad/.test(navigator.platform) ? '⌘↵' : 'Ctrl+↵';
    this.hint.textContent = question ? `The question stays open. ${keys}` : keys;
  }

  /** Quotes `text` in the message, or drops the quote with `null`.
   * @param {string | null} text */
  quote(text) {
    this.quoted.value = text ?? '';
    this.quoteText.textContent = text ? `“${text}”` : '';
    this.quoteChip.hidden = !text;
    keep(this.quoteKey, text || null);
  }

  focus() {
    if (!this.element.hidden) this.box.focus({ preventScroll: true });
  }
}

/**
 * One message: the reviewer's, with where it was written and its quote; or the agent's, on the
 * panel's surface.
 * @param {ChatMessageView} message
 */
function message(message) {
  const mine = message.author === 'reviewer';
  const eyebrow = [mine ? 'You' : 'Agent', mine ? message.place : null, message.posted_at_ms === null ? null : clockTime(message.posted_at_ms)]
    .filter((part) => part)
    .join(' · ');
  return h(
    'article',
    { class: `chat-message ${mine ? 'mine' : 'agent'}`, 'aria-label': mine ? 'Your message' : 'Reply from the agent' },
    h('p', { class: 'eyebrow' }, eyebrow),
    mine && message.quote ? h('blockquote', { class: 'chat-quote-shown' }, message.quote) : null,
    markdown(message.html, 'chat-text-shown'),
  );
}

/**
 * The row that says the agent works on the reviewer's messages, with the time since the
 * earliest one it has not replied to.
 * @param {number} since
 */
function pendingRow(since) {
  return h(
    'p',
    { class: 'chat-pending', role: 'status' },
    h('span', { class: 'chat-pending-glyph', 'aria-hidden': 'true' }, '…'),
    h('span', {}, 'The agent is answering · ', h('span', { class: 'chat-pending-time', 'data-since': String(since) }, elapsed(since))),
  );
}

/**
 * When the earliest of the reviewer's messages that wait for the agent was posted; `null` when
 * none waits, or a message that waits did not reach the agent.
 * @param {ChatMessageView[]} messages
 */
function waitingSince(messages) {
  if (messages.some((one) => one.delivery?.state === 'not_delivered')) return null;
  const waiting = messages.filter((one) => one.delivery?.state === 'waiting');
  if (waiting.length === 0) return null;
  return waiting[0].posted_at_ms ?? Date.now();
}

/** "0:31": the minutes and seconds since `since`.
 * @param {number} since */
function elapsed(since) {
  const seconds = Math.max(0, Math.floor((Date.now() - since) / 1000));
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`;
}

/** A new message identity: a version 4 UUID, which the page's address on the network, not a
 * secure context, can make too. */
function messageId() {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = [...bytes].map((byte) => byte.toString(16).padStart(2, '0')).join('');
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

/**
 * The phone sheet's grabber: a drag up opens the sheet to the full height, a drag down brings it
 * back to two thirds, or closes it.
 * @param {HTMLElement} grabber
 * @param {Chat} chat
 */
function dragSheet(grabber, chat) {
  /** @type {number | null} */
  let from = null;
  grabber.addEventListener('pointerdown', (event) => {
    from = event.clientY;
    grabber.setPointerCapture(event.pointerId);
  });
  grabber.addEventListener('pointerup', (event) => {
    if (from === null) return;
    const moved = event.clientY - from;
    from = null;
    const full = chat.element.classList.contains('full');
    if (moved < -DRAG) chat.element.classList.add('full');
    else if (moved > DRAG && full) chat.element.classList.remove('full');
    else if (moved > DRAG) chat.close(false);
  });
}
