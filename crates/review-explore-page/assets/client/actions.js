// The reviewer's actions: every form of the page names the request it sends (`data-method`),
// and one listener turns its submit into that request on the socket, with the identities its
// hidden fields carry. A form waits for its reply, so a second click sends nothing; while the
// socket is down, every action waits, its button disabled. A refusal shows its notice; what an
// action changed arrives as the next view, before its reply.

/** @import { Call, Reply } from "./types.ts" */
/** @import { Link } from "./socket.js" */
/** @import { Page } from "./page.js" */

/**
 * @typedef {{ form: HTMLFormElement, data: FormData, submitter: HTMLElement | null }} Submitted
 */

/** How each form's fields make its request. @type {Record<string, (submitted: Submitted) => Call>} */
const CALLS = {
  answer: ({ data }) => ({
    method: 'answer',
    params: {
      round: optional(data, 'round'),
      question: text(data, 'question'),
      version: Number(text(data, 'version')),
      choice: optional(data, 'choice'),
      comment: text(data, 'comment'),
    },
  }),
  pick: ({ data }) => ({
    method: 'pick',
    params: {
      round: optional(data, 'round'),
      question: text(data, 'question'),
      version: Number(text(data, 'version')),
      choice: text(data, 'choice'),
    },
  }),
  start: ({ data, submitter }) => ({
    method: 'start',
    params: {
      challenger: submitter instanceof HTMLButtonElement && submitter.value === 'true',
      start: text(data, 'start'),
    },
  }),
  stop: ({ data }) => ({
    method: 'stop',
    params: { start: optional(data, 'start'), request: optional(data, 'request') },
  }),
  retry: ({ data }) => ({
    method: 'retry',
    params: { request: text(data, 'request'), attempt: text(data, 'attempt') },
  }),
  'cancel-answer': ({ data }) => ({ method: 'cancel-answer', params: { answer: text(data, 'answer') } }),
  reset: ({ data }) => ({ method: 'reset', params: { round: text(data, 'round') } }),
  implement: ({ data }) => ({
    method: 'implement',
    params: {
      conclusion: text(data, 'conclusion'),
      replaces: optional(data, 'replaces'),
      text: text(data, 'text'),
    },
  }),
  quiz: ({ data }) => ({
    method: 'quiz',
    params: {
      conclusion: text(data, 'conclusion'),
      item: Number(text(data, 'item')),
      answer: Number(text(data, 'answer')),
    },
  }),
  'quiz-skip': ({ data }) => ({ method: 'quiz-skip', params: { conclusion: text(data, 'conclusion') } }),
  reply: ({ data }) => ({
    method: 'reply',
    params: { conclusion: text(data, 'conclusion'), text: text(data, 'text') },
  }),
  'cancel-implementation': ({ data }) => ({
    method: 'cancel-implementation',
    params: { delivery: text(data, 'delivery') },
  }),
  'resend-implementation': ({ data }) => ({
    method: 'resend-implementation',
    params: {
      conclusion: text(data, 'conclusion'),
      delivery: text(data, 'delivery'),
      attempt: text(data, 'attempt'),
    },
  }),
};

/**
 * @param {FormData} data
 * @param {string} name
 */
function text(data, name) {
  const value = data.get(name);
  return typeof value === 'string' ? value : '';
}

/**
 * @param {FormData} data
 * @param {string} name
 */
function optional(data, name) {
  const value = data.get(name);
  return typeof value === 'string' ? value : null;
}

export class Actions {
  /**
   * @param {HTMLElement} main
   * @param {Link} link
   * @param {Page} page
   */
  constructor(main, link, page) {
    this.main = main;
    this.link = link;
    this.page = page;
    this.connected = false;
    // The forms of the masthead (Reset, in its menu) are outside `main`.
    main.ownerDocument.addEventListener('submit', (event) => this.submit(event));
  }

  /** @param {SubmitEvent} event */
  async submit(event) {
    const form = event.target;
    if (!(form instanceof HTMLFormElement)) return;
    event.preventDefault();
    const make = CALLS[form.dataset.method ?? ''];
    if (!make || form.dataset.sent === 'true' || !this.connected) return;
    const call = make({ form, data: new FormData(form), submitter: event.submitter });
    this.sending(form, true);
    this.page.showNotice(null);
    try {
      this.settle(call, await this.link.call(call));
    } catch {
      // The socket closed before the reply: the next view shows what became of the action.
    } finally {
      this.sending(form, false);
    }
  }

  /**
   * @param {Call} call
   * @param {Reply} reply
   */
  settle(call, reply) {
    if ('error' in reply) {
      this.page.showNotice(reply.error.data);
      return;
    }
    // A pick shows the item's answer until the reviewer moves on; a skip shows the conclusion.
    if (call.method === 'quiz' || call.method === 'quiz-skip') {
      this.page.answered =
        call.method === 'quiz' ? { conclusion: call.params.conclusion, item: call.params.item } : null;
      if (this.page.view) this.page.render(this.page.view);
    }
    // A Reset on the network ends the page's token: the page opens again with the next one.
    if (reply.result.reopen !== null) location.replace(`/?token=${encodeURIComponent(reply.result.reopen)}`);
  }

  /**
   * Marks `form` as waiting for its reply, or as done.
   * @param {HTMLFormElement} form
   * @param {boolean} waiting
   */
  sending(form, waiting) {
    form.dataset.sent = String(waiting);
    if (waiting) this.main.setAttribute('aria-busy', 'true');
    else if (!this.main.ownerDocument.querySelector('form[data-sent="true"]')) this.main.removeAttribute('aria-busy');
    this.enable();
  }

  /** @param {boolean} connected whether the socket is open */
  connection(connected) {
    this.connected = connected;
    this.enable();
  }

  /** Enables each submit button unless its form waits, the socket is down, or the round blocks
   * it (`data-blocked`). Called after each render too. */
  enable() {
    for (const button of this.main.ownerDocument.querySelectorAll('button[type="submit"]')) {
      if (!(button instanceof HTMLButtonElement)) continue;
      const form = button.form;
      button.disabled = !this.connected || form?.dataset.sent === 'true' || button.dataset.blocked === 'true';
    }
  }
}
