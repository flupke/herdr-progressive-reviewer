// The reviewer's actions: every form of the page names the request it sends (`data-method`),
// and one listener turns its submit into that request on the socket, with the identities its
// hidden fields carry. A form waits for its reply, so a second click sends nothing; while the
// socket is down, every action waits, its button disabled. A form may also say what it needs
// before it can be sent (`data-requires`): its button stays dimmed until then. A refusal shows
// its notice; what an action changed arrives as the next view, before its reply, and the page
// then brings the part that changed into view (`CHANGED`).

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
      number: number(data, 'number'),
    },
  }),
  pick: ({ data }) => ({
    method: 'pick',
    params: {
      round: optional(data, 'round'),
      question: text(data, 'question'),
      version: Number(text(data, 'version')),
      choice: text(data, 'choice'),
      number: number(data, 'number'),
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
  'send-message': ({ data }) => ({
    method: 'send-message',
    params: {
      round: text(data, 'round'),
      id: text(data, 'id'),
      text: text(data, 'text'),
      asked_under: askedUnder(data),
      quote: optional(data, 'quote') || null,
    },
  }),
  'retry-messages': ({ data }) => ({ method: 'retry-messages', params: { round: text(data, 'round') } }),
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

/** Where the result of each action shows, which the page brings into view when the reviewer
 * cannot see it: the element its module marks with `data-shows` (the reveal of the
 * recommendation after a first pick, the verdict on a quiz answer, the question waiting again
 * after Cancel answer), or the round's state, a status card, after an answer. @type {Record<string, string>} */
const CHANGED = {
  pick: '[data-shows="pick"]',
  quiz: '[data-shows="quiz"]',
  answer: '.status-card',
  'cancel-answer': '[data-shows="cancel-answer"]',
};

/** What each value of `data-requires` asks of a form before it can be sent.
 * @type {Record<string, (form: HTMLFormElement) => boolean>} */
const REQUIRES = {
  choice: (form) => form.querySelector('input[name="choice"]:checked') !== null,
  answer: (form) => form.querySelector('input[name="answer"]:checked') !== null,
  'choice-or-comment': (form) =>
    REQUIRES.choice(form) || text(new FormData(form), 'comment').trim() !== '',
  text: (form) => text(new FormData(form), 'text').trim() !== '',
};

/**
 * Where in the round the chat's message was written, from its form's fields.
 * @param {FormData} data
 * @returns {import('./types.ts').AskedUnder | null}
 */
function askedUnder(data) {
  switch (text(data, 'stage')) {
    case 'question':
      return { stage: 'question', question: text(data, 'question'), version: Number(text(data, 'version')) };
    case 'design':
      return { stage: 'design' };
    case 'conclusion':
      return { stage: 'conclusion', conclusion: text(data, 'conclusion') };
    default:
      return null;
  }
}

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

/**
 * A whole number the form carries, or `null` when it carries none.
 * @param {FormData} data
 * @param {string} name
 */
function number(data, name) {
  const value = optional(data, name);
  return value === null || value === '' || Number.isNaN(Number(value)) ? null : Number(value);
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
    // An edit may block or unblock its form's button (`data-blocked`), in the chat too, which
    // is outside `main`.
    main.ownerDocument.addEventListener('input', () => this.enable());
  }

  /** @param {SubmitEvent} event */
  async submit(event) {
    const form = event.target;
    if (!(form instanceof HTMLFormElement)) return;
    event.preventDefault();
    const make = CALLS[form.dataset.method ?? ''];
    if (!make || form.dataset.sent === 'true' || !this.connected || !ready(form)) return;
    const call = make({ form, data: new FormData(form), submitter: event.submitter });
    this.sending(form, true);
    this.page.showNotice(null);
    try {
      const reply = await this.link.call(call);
      this.settle(call, reply);
      // The form's own module may follow what went through: the chat clears its composer.
      if ('result' in reply) form.dispatchEvent(new CustomEvent('sent'));
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
    if (reply.result.reopen !== null) {
      location.replace(`/?token=${encodeURIComponent(reply.result.reopen)}`);
      return;
    }
    showChange(this.main, call.method);
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

  /** Enables each submit button unless its form waits, the socket is down, the round blocks it
   * (`data-blocked`), or its form lacks what it requires. Called after each render and each edit
   * too. */
  enable() {
    for (const button of this.main.ownerDocument.querySelectorAll('button[type="submit"]')) {
      if (!(button instanceof HTMLButtonElement)) continue;
      const form = button.form;
      button.disabled =
        !this.connected ||
        form?.dataset.sent === 'true' ||
        button.dataset.blocked === 'true' ||
        (form !== null && !ready(form));
    }
  }
}

/** Whether `form` has what its `data-requires` asks for.
 * @param {HTMLFormElement} form */
function ready(form) {
  const requires = form.dataset.requires;
  return !requires || (REQUIRES[requires]?.(form) ?? true);
}

/**
 * Brings the result of the action `method` into view, when it is out of view, and gives it the
 * focus when it can take it.
 * @param {HTMLElement} main
 * @param {string} method
 */
function showChange(main, method) {
  const selector = CHANGED[method];
  const changed = selector ? main.querySelector(selector) : null;
  if (!(changed instanceof HTMLElement)) return;
  const box = changed.getBoundingClientRect();
  if (box.top < 0 || box.top > window.innerHeight - 48) changed.scrollIntoView({ block: 'start' });
  if (changed.tabIndex >= 0 || changed.hasAttribute('tabindex')) changed.focus({ preventScroll: true });
}
