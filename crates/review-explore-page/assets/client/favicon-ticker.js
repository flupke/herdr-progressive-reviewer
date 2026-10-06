// The timer of the tab's busy icon (favicon.js), in a worker, which a browser throttles less
// than a hidden page: a message of N milliseconds starts a tick every N milliseconds, 0 stops it.

/** The worker's scope, as far as the timer needs it. */
const scope = /** @type {{ postMessage(message: unknown): void, addEventListener(type: 'message', listener: (event: MessageEvent) => void): void }} */ (
  /** @type {unknown} */ (globalThis)
);

/** @type {ReturnType<typeof setInterval> | undefined} */
let timer;

scope.addEventListener('message', (event) => {
  clearInterval(timer);
  const ms = Number(event.data);
  if (ms > 0) timer = setInterval(() => scope.postMessage(0), ms);
});

export {};
