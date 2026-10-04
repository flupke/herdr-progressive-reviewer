// Follows the round in every stage: polls the page's status, and loads the page again once the
// round has changed, or once the page's address no longer opens a round (a Reset in the pane
// closed it). The poll compares a revision only. It asks more often while a round starts, the
// agent works, or an implementation request is being sent.
(() => {
  const round = document.getElementById('round');
  if (!round) return;
  const revision = Number(round.dataset.revision);
  const interval = round.dataset.working === 'true' ? 500 : 1500;

  async function poll() {
    try {
      const response = await fetch('/status', { cache: 'no-store' });
      if (response.status === 403 || (response.ok && (await response.json()).revision !== revision)) {
        location.reload();
        return;
      }
    } catch {
      // The server is restarting, or cannot be reached: ask again.
    }
    setTimeout(poll, interval);
  }
  setTimeout(poll, interval);
})();

// Sends an answer, an implementation request, a quiz pick, a reply, a Retry or a Reset once: a
// second click would post it again, which the tool refuses since the round moved on already.
(() => {
  for (const form of document.querySelectorAll('form.answer, form.implement, form.quiz-pick, form.reply, form.retry, form.reset')) {
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

// Keeps what the reviewer types in a form's text box across the loads that follow the round:
// a load the round's change caused, or a load after a refused post, shows the text again in the
// same form of the same question, conclusion or round, for as long as the tab stays open. The
// form's identity is its action and the identities its hidden fields post, so a later question
// or conclusion starts empty.
(() => {
  const storage = (() => {
    try {
      return window.sessionStorage;
    } catch {
      return null;
    }
  })();
  if (!storage) return;
  const key = (form, box) => {
    const hidden = [...form.querySelectorAll('input[type=hidden]')].map((input) => `${input.name}=${input.value}`);
    return `explore-draft:${form.getAttribute('action')}:${hidden.join('&')}:${box.name}`;
  };
  try {
    for (const form of document.querySelectorAll('form')) {
      for (const box of form.querySelectorAll('textarea')) {
        const name = key(form, box);
        const kept = storage.getItem(name);
        if (kept !== null) box.value = kept;
        box.addEventListener('input', () => storage.setItem(name, box.value));
      }
    }
  } catch {
    // The browser keeps nothing for this page: the text is lost on a load.
  }
})();
