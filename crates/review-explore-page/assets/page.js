// While a round starts, the agent works, or an implementation request is being sent, polls the
// page's status, and loads the page again once the round has changed. In every other state the
// page changes only when it is loaded again.
(() => {
  const round = document.getElementById('round');
  if (round.dataset.polls !== 'true') return;
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

// Sends an answer, an implementation request or a quiz pick once: a second click would post it
// again, which the tool refuses since the question has an answer, or the conclusion a request,
// already.
(() => {
  for (const form of document.querySelectorAll('form.answer, form.implement, form.quiz-pick')) {
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
