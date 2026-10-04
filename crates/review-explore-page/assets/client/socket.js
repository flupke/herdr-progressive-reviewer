// The page's one socket to the review tool, at /ws (crate::socket, crate::rpc). The tool sends
// the whole view when the socket opens and at each change; the page sends the reviewer's actions
// as requests in the shape of JSON-RPC 2.0, each answered once, matched by its `id`.
//
// What is ours, beside the browser's WebSocket: reconnection with a capped exponential back-off
// with full jitter, a watchdog that opens a new socket when the tool's pings stop (a laptop that
// slept, a phone that changed networks), and an immediate check when the page is shown again or
// the network comes back. Nothing is replayed after a reconnect: the new socket gets the current
// view. Actions wait while the socket is down; none is queued to be sent later.

/**
 * @import { Call, Reply, Notification, StateParams } from "./types.ts"
 */

/** The tool pings every 10 seconds (crate::socket::HEARTBEAT): a socket silent for longer than
 * this is dead. */
const SILENCE = 25_000;
/** How long a check of a socket that should be alive waits for the tool's reply. */
const CHECK = 3_000;
/** The back-off: the first delay, and the longest. */
const FIRST_DELAY = 250;
const LONGEST_DELAY = 10_000;
/** A socket that stayed open this long resets the back-off. */
const STABLE = 5_000;
/** The close code of a socket whose token no longer opens a round (crate::socket::TOKEN_ENDED). */
const TOKEN_ENDED = 4001;

/**
 * @typedef {'connecting' | 'open' | 'down' | 'ended'} LinkState
 * @typedef {{ resolve: (reply: Reply) => void, reject: (error: Error) => void }} Pending
 */

export class Link {
  /**
   * @param {{ state: (params: StateParams) => void, link: (state: LinkState) => void }} listener
   */
  constructor(listener) {
    this.listener = listener;
    /** @type {WebSocket | null} */
    this.socket = null;
    /** @type {LinkState} */
    this.state = 'connecting';
    this.attempts = 0;
    this.lastHeard = Date.now();
    this.nextId = 1;
    /** @type {Map<number, Pending>} */
    this.pending = new Map();
    /** @type {number | undefined} */
    this.retryTimer = undefined;
    /** @type {number | undefined} */
    this.stableTimer = undefined;
  }

  /** Opens the socket, and keeps one open until the token ends. */
  start() {
    this.connect();
    setInterval(() => this.watch(), 5_000);
    const check = () => this.check();
    document.addEventListener('visibilitychange', () => {
      if (document.visibilityState === 'visible') check();
    });
    window.addEventListener('pageshow', (event) => {
      if (event.persisted) this.connect();
    });
    window.addEventListener('online', check);
    window.addEventListener('focus', check);
    // A page with an open socket is not kept in the back-forward cache by every browser.
    window.addEventListener('pagehide', () => this.socket?.close(1000, 'The page is hidden'));
  }

  connect() {
    if (this.state === 'ended') return;
    clearTimeout(this.retryTimer);
    if (this.socket && this.socket.readyState <= WebSocket.OPEN) return;
    const scheme = location.protocol === 'https:' ? 'wss:' : 'ws:';
    const socket = new WebSocket(`${scheme}//${location.host}/ws`);
    this.socket = socket;
    this.lastHeard = Date.now();
    socket.addEventListener('open', () => {
      if (socket !== this.socket) return;
      this.setState('open');
      this.stableTimer = window.setTimeout(() => {
        this.attempts = 0;
      }, STABLE);
    });
    socket.addEventListener('message', (event) => {
      if (socket !== this.socket) return;
      this.lastHeard = Date.now();
      this.receive(String(event.data));
    });
    socket.addEventListener('close', (event) => {
      if (socket !== this.socket) return;
      this.socket = null;
      clearTimeout(this.stableTimer);
      this.failPending();
      if (event.code === TOKEN_ENDED) {
        this.setState('ended');
        return;
      }
      this.setState('down');
      this.retry(event.wasClean);
    });
  }

  /**
   * Sends `call` and waits for its reply; rejects when the socket is down, or closes first.
   * @param {Call} call
   * @returns {Promise<Reply>}
   */
  call(call) {
    const socket = this.socket;
    if (!socket || socket.readyState !== WebSocket.OPEN) {
      return Promise.reject(new Error('The socket is down'));
    }
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject });
      socket.send(JSON.stringify({ id, ...call }));
    });
  }

  /** @param {string} text */
  receive(text) {
    /** @type {Reply | Notification} */
    const message = JSON.parse(text);
    if ('id' in message) {
      const pending = this.pending.get(message.id);
      this.pending.delete(message.id);
      pending?.resolve(message);
    } else if (message.method === 'state') {
      this.listener.state(message.params);
    }
  }

  /**
   * Opens the next socket after a delay: full jitter on a doubling, capped back-off. A socket
   * refused at its upgrade may have a token that ended: the page asks the tool before it tries
   * again.
   * @param {boolean} wasOpen whether the socket closed cleanly
   */
  retry(wasOpen) {
    const ceiling = Math.min(LONGEST_DELAY, FIRST_DELAY * 2 ** this.attempts);
    this.attempts++;
    const delay = Math.random() * ceiling;
    this.retryTimer = window.setTimeout(async () => {
      if (!wasOpen && (await this.tokenEnded())) {
        this.setState('ended');
        return;
      }
      this.connect();
    }, delay);
  }

  /** Whether the tool answers, and says that the page's token opens no round any more. */
  async tokenEnded() {
    try {
      const response = await fetch('/token', { cache: 'no-store' });
      return response.status === 403;
    } catch {
      return false;
    }
  }

  /** Opens a new socket when the tool has been silent for too long. */
  watch() {
    if (this.state === 'open' && Date.now() - this.lastHeard > SILENCE) this.reopen();
  }

  /** Checks at once that the socket is alive: the page is shown again, or the network is back. */
  async check() {
    if (this.state === 'ended') return;
    if (this.state !== 'open') {
      this.attempts = 0;
      this.connect();
      return;
    }
    const alive = await Promise.race([
      this.call({ method: 'ping' }).then(
        () => true,
        () => false,
      ),
      new Promise((resolve) => setTimeout(() => resolve(false), CHECK)),
    ]);
    if (!alive) this.reopen();
  }

  /** Drops a socket that seems dead, and opens a new one at once. */
  reopen() {
    const socket = this.socket;
    this.socket = null;
    socket?.close(4000, 'No word from the review tool');
    this.failPending();
    this.setState('down');
    this.attempts = 0;
    this.connect();
  }

  failPending() {
    for (const pending of this.pending.values()) pending.reject(new Error('The socket closed'));
    this.pending.clear();
  }

  /** @param {LinkState} state */
  setState(state) {
    if (this.state === state) return;
    this.state = state;
    this.listener.link(state);
  }
}
