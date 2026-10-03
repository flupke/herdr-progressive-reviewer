// While the agent works, polls the page's status, and loads the page again once the round has
// changed. In every other state the page changes only when it is loaded again.
(() => {
  const round = document.getElementById('round');
  if (round.dataset.working !== 'true') return;
  const revision = Number(round.dataset.revision);

  async function poll() {
    try {
      const response = await fetch('/status', { cache: 'no-store' });
      if (response.ok) {
        const status = await response.json();
        if (status.revision !== revision) {
          location.reload();
          return;
        }
      }
    } catch {
      // The server is restarting: ask again.
    }
    setTimeout(poll, 500);
  }
  setTimeout(poll, 500);
})();
