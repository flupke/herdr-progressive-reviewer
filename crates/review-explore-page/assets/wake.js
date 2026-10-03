// When the page is shown again (a phone that wakes, a tab brought back, a page the browser
// restores from its cache), loads it again if the round changed in between, or if the page's
// address no longer opens a round: the reviewer started a new round or closed this one.
(() => {
  const round = document.getElementById('round');
  if (!round) return;
  const revision = Number(round.dataset.revision);

  async function check() {
    try {
      const response = await fetch('/status', { cache: 'no-store' });
      const refused = response.status === 403;
      if (refused || (response.ok && (await response.json()).revision !== revision)) {
        location.reload();
      }
    } catch {
      // The reviewer cannot be reached: the page stays as it is.
    }
  }

  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'visible') check();
  });
  window.addEventListener('pageshow', (event) => {
    if (event.persisted) check();
  });
  window.addEventListener('online', check);
})();
