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
  box.dataset.draft = key;
  box.value = kept(key) ?? initial;
  box.addEventListener('input', () => keep(key, box.value));
  return box;
}

/**
 * The value kept under `key` for the tab, if any: a draft, or what goes with it (the chat's
 * quote).
 * @param {string} key
 * @returns {string | null}
 */
export function kept(key) {
  try {
    return storage?.getItem(`explore-draft:${key}`) ?? null;
  } catch {
    // The browser keeps nothing for this page.
    return null;
  }
}

/**
 * Keeps `value` under `key` for the tab, or forgets it with `null`.
 * @param {string} key
 * @param {string | null} value
 */
export function keep(key, value) {
  try {
    if (value === null) storage?.removeItem(`explore-draft:${key}`);
    else storage?.setItem(`explore-draft:${key}`, value);
  } catch {
    // The browser keeps nothing for this page: the text is lost on a rebuild.
  }
}
