// Development only: loads the page again when a template or an asset changes on disk. The
// server answers /dev/changes from its file watcher, so nothing polls the files.
(() => {
  const version = Number(document.currentScript.dataset.version);
  async function wait() {
    try {
      const response = await fetch(`/dev/changes?since=${version}`, { cache: 'no-store' });
      const changes = await response.json();
      if (changes.version !== version) {
        location.reload();
        return;
      }
    } catch {
      await new Promise((resolve) => setTimeout(resolve, 1000));
    }
    wait();
  }
  wait();
})();
