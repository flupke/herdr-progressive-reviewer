// The keys that send a text box's form from the box, ⌘↵ or Ctrl+↵: the chat's message and the
// comment of an answer.

/** The keys, as the page names them. */
export const SEND_KEYS_LABEL = /Mac|iPhone|iPad/.test(navigator.platform) ? '⌘↵' : 'Ctrl+↵';

/**
 * Makes the keys in `box` press its form's submit button, when the button is enabled.
 * @template {HTMLTextAreaElement} B
 * @param {B} box
 * @returns {B}
 */
export function sendsOnKeys(box) {
  box.setAttribute('aria-keyshortcuts', 'Control+Enter Meta+Enter');
  box.addEventListener('keydown', (event) => {
    if (event.key !== 'Enter' || !(event.metaKey || event.ctrlKey) || event.isComposing) return;
    event.preventDefault();
    // A disabled button means the form cannot be sent now (it waits for its reply, the socket is
    // down, or it lacks what it requires): the keys then send nothing, and a blind question's
    // unpicked choices show no bubble of the browser.
    const send = box.form?.querySelector('button[type="submit"]');
    if (send instanceof HTMLButtonElement && !send.disabled) box.form?.requestSubmit(send);
  });
  return box;
}
