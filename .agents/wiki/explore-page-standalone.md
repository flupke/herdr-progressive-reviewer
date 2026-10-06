# Explore page: standalone server and screenshot gallery

How to serve the page with no pane, agent or Herdr, and how to shoot every state of it for a before/after comparison.

## Serve the page alone

```sh
nix develop --command make explore-page
```

This runs the standalone server
([`crates/review-explore-page-server`](../../crates/review-explore-page-server)),
with no pane, no agent and no Herdr. It prints the address of a page that shows
a fixed question: `http://127.0.0.1:8790/?token=dev`. Open it in a browser.

The server reads the shell, the client's modules and the stylesheets from disk on
every request, and an open page loads itself again when one of them changes (its
file watcher answers `/dev/changes`; nothing polls the files), so a client or style
edit shows at once. A Rust edit needs a rebuild: stop the server and run the command
again; the open page reconnects by itself once the server is back. The token stays
`dev`, so the page opens again without a new address.

The server's own options: `--port N` (`0` picks a free port), `--token T`
(random when omitted), `--data short|rich`, what the agent posts, `--dev DIR`, the
page crate's directory to read the files from, and `--fixed-clock`, which stamps every start
and turn with the same time, and the chat's messages one second apart from that time (the
gallery uses it, and holds its pages' clock 42 seconds later, so that a time since reads
"0:42" in every run). Without `--dev`, it serves the files built
into the binary. The `short` data set, the default, is the one the e2e tests check; the
`rich` one is as long as a real round (a design in four full parts, questions with several
paragraphs of Context, tables, diagrams, three citations of three files of a change of 43
files, most of them under one deep directory, a one-way question, a diagram that does not parse, a conclusion with a ten-line list and a
quiz of three items), for the gallery below; `make explore-page
EXPLORE_PAGE_ARGS='--data rich'` serves it. Both are in
[`round_data.rs`](../../crates/review-explore-page-server/src/round_data.rs). A test of the
rich round, such as the meter's with the 43 files, is registered with `richTest` from
`tests/explore-page/tests/session.ts` instead of `test`: its session's agent posts the rich
data set whatever the server's `--data`.

## Screenshot gallery

```sh
nix develop --command make explore-gallery
```

This takes a full-page screenshot of every state of the page, at 1280 and 390
pixels wide, in the light and the dark theme, and writes them with a contact
sheet, `index.html`, which shows each state's screenshots side by side. It starts
the standalone server with the rich data set and its fixed clock and, for each screenshot,
moves a fresh session of it to the state, in a page loaded at that width and theme, in the
UTC time zone and with its clock held still 42 seconds after the server's, through the
e2e fixture's helpers (`openSession` in `tests/explore-page/tests/session.ts`) and
exact actions on the page. It waits until the client has drawn the round, with its fonts
and diagrams, and has the reply to the last action it sent, then makes the window as tall
as the page, so that the
sticky column of the reviewer's actions shows whole rather than scrolling inside
itself, and shoots with the dev shell's headless Chromium. It calls no model, needs
no network once the npm packages are installed, and is not part of `make check`. A
state that fails to reach its page stops the run with its name.

Each image is named `<state>-<width>-<theme>.png`, so two runs compare file by file.
The variables, all optional:

- `GALLERY_DIR`: the folder to write, new or empty. By default a new folder under the
  system's temporary directory, which the run prints.
- `GALLERY_COMPARE`: an earlier gallery's folder. The contact sheet then shows before
  and after for each image that differs, marks the new ones, lists the ones that are
  gone, and can hide the images that did not change; the run prints the counts.
- `GALLERY_WIDTHS`: other widths, separated by spaces (`GALLERY_WIDTHS='1600 1280 900 390'`).
- `GALLERY_STATES`: only these states, by name, separated by spaces.

Two runs on the same code give the same files, on any machine: the fixed clocks and the time
zone make every time the page shows the same; an image differs only where the
browser draws differently. The comparison is byte for byte, so such a difference
also counts as a change. The states are listed once, in
[`tests/explore-page/gallery/states.ts`](../../tests/explore-page/gallery/states.ts),
in the order of the contact sheet. To add a state of the page, add one entry there:
a name, which never changes once given, since it names the files; a line that says
what the state shows; and `reach`, which moves a fresh session, whose agent works on
its first question, to the state with the fixture's helpers, and leaves the page
showing it; the helpers of that file cover the usual paths (`after` for a move of the
round, `question`, `conclusion`). A state whose layout depends on a wide window names the
extra widths it is shot at in `extraWidths` (the `working` state is also shot at 2000
pixels). Before an action the round no longer offers, hold the page (`refused` in that file) so that it does not follow the round first. A state
that needs a new move of the round needs a control route of the server first, as
for an e2e test.
