# README screenshots

Where the README's screenshots in `docs/screenshots/` come from, and the gotchas of taking them
again.

All of them show one change, the rich data set of the standalone page server ("Batch the
reviewer's replies into one agent notification", `crates/review-explore-page-server/src/rich/`),
so the pane and the Explore page tell the same story. The pane comes first: the README sells
progressive reviews and review threads, and Explore after them.

## The pane

`pane-review` shows a file that changed since review, with the agent's reply in its thread;
`pane-threads` shows the Threads tab.

- Take them with the vision server ([Terminal UI exploration](tui-vision.md)). Replace its
  scratch repository's changes with two jj changes: the rich data set's `.old` files
  (`jj new 'root()'`), then its `.new` files on top (`jj new`); the reviewer follows the new
  working copy.
- The scene: mark `CHANGELOG.md` and `flush.rs` reviewed; comment on the `notify_batch` line of
  `queue.rs` and mark it reviewed; comment on a line of `reply.rs`, left unanswered; then, as the
  agent, add a line to `queue.rs` and answer the first comment. `queue.rs` then shows its
  reviewed hunks folded and the new line "changed since review".
- The pane takes the width of the user's terminal, often too wide to read at the README's
  width: before the shot, narrow it to about 120 columns with
  `herdr pane resize --pane <reviewer pane> --direction left --amount <ratio>`; `herdr pane
  layout` gives the split's width. Its height stays the terminal's: cut the empty rows out of
  the image, between the last used row and the pane's bottom border.
- Write the agent's reply from a script, not a shell command line: the shell runs the backticks
  of its inline code.
- The screenshot draws the thread mark 💬 as `??`. Paint the emoji from Noto Color Emoji over its
  two cells. tui-test draws a cell in 10×21 pixels with its bundled font, from a margin of 39
  pixels at the left and 62 at the top.
- The window's title bar names the harness, "tui-test capture - <columns>x<rows>": paint the
  title over with the bar's color.

## The Explore page

`explore-question` is the gallery state `question-1` (`tests/explore-page/gallery/states.ts`),
in a 1280×800 window.

- Shoot it with a throwaway `make explore-script` script
  ([standalone page](explore-page-standalone.md)): start a `StandaloneServer` with
  `--data rich --fixed-clock`, open a browser context with `deviceScaleFactor: 2` for each
  theme, run the state's `reach`, and take a screenshot of the window, not of the whole page.
- Park the pointer in a corner before the shot: where the state's last click leaves it, the
  pointer can rest on the meter and open its window.
- Save it as lossless WebP: about 140 KB at 2x, a third of the PNG, and smaller than lossy WebP
  for this flat screen. It has a light and a dark file, which the README picks with
  `<picture>`.
