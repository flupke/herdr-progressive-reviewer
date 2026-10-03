// While a round starts or the agent works, polls the page's status, and loads the page
// again once the round has changed. In every other state the page changes only when it is loaded
// again.
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

// Sends an answer once: a second click would post the same answer again, which the tool
// refuses since the question already has one.
(() => {
  for (const form of document.querySelectorAll('form.answer')) {
    form.addEventListener('submit', (event) => {
      if (form.dataset.sent === 'true') {
        event.preventDefault();
        return;
      }
      form.dataset.sent = 'true';
      for (const button of form.querySelectorAll('button[type=submit]')) button.disabled = true;
    });
  }
})();

// Starts a round once: a second click would ask for a second start, which the tool refuses.
// The buttons stay enabled, since a disabled button would not send its Challenger choice.
(() => {
  for (const form of document.querySelectorAll('form.start')) {
    form.addEventListener('submit', (event) => {
      if (form.dataset.sent === 'true') event.preventDefault();
      form.dataset.sent = 'true';
    });
  }
})();
