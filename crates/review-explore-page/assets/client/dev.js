// Development only: loads the page again when a file of the page changes on disk. The server
// answers /dev/changes from its file watcher, so nothing polls the files.

/** @param {number} version the count of file changes the page was loaded at */
export async function reloadOnChange(version) {
  for (;;) {
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
  }
}
