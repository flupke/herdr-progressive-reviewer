// What the reviewer types in a text box, kept for as long as the tab stays open: a text box
// rebuilt for the same question, conclusion or round shows it again, and so does the page after
// a reload. The draft's key names what the text is for (the action and the identities it acts
// on), so a later question starts empty.

const storage = (() => {
  try {
    return window.sessionStorage;
  } catch {
    return null;
  }
})();

/**
 * Makes `box` keep its text under `key`: it starts with the kept text, or `initial` when none is
 * kept, and keeps each edit. The key also names the box (`data-draft`), so that the page gives
 * the focus back to it after a rebuild.
 * @param {HTMLTextAreaElement} box
 * @param {string} key
 * @param {string} initial
 * @returns {HTMLTextAreaElement}
 */
export function keepDraft(box, key, initial) {
  const name = `explore-draft:${key}`;
  box.dataset.draft = key;
  let kept = null;
  try {
    kept = storage?.getItem(name) ?? null;
  } catch {
    // The browser keeps nothing for this page.
  }
  box.value = kept ?? initial;
  box.addEventListener('input', () => {
    try {
      storage?.setItem(name, box.value);
    } catch {
      // The browser keeps nothing for this page: the text is lost on a rebuild.
    }
  });
  return box;
}
