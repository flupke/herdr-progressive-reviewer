# Explore page: standalone server and screenshot gallery

How to serve the page with no pane, agent or Herdr, act on it with a throwaway script, and shoot every state of it for a before/after comparison.

## Serve the page alone

```sh
nix develop --command make explore-page
```

This runs the standalone server
([`crates/review-explore-page-server`](../../crates/review-explore-page-server)),
with no pane, no agent and no Herdr. It prints the address of a page that shows
a fixed question: `http://127.0.0.1:8790/?token=dev`. Open it in a browser.

The server reads the shell, the client's modules and the stylesheets from disk on
every request, and an open page loads itself again when one of them changes, so a client or style
edit shows at once. A Rust edit needs a rebuild: stop the server and run the command
again; the open page reconnects by itself once the server is back. The token stays
`dev`, so the page opens again without a new address.

The server's options are listed at the top of
[`main.rs`](../../crates/review-explore-page-server/src/main.rs). `--data rich` serves a
round as long as a real one, for the gallery below; `make explore-page
EXPLORE_PAGE_ARGS='--data rich'` serves it.

## Act on the page with a script

```sh
nix develop --command make explore-script SCRIPT=path/to/script.ts
```

This starts a fresh standalone server, opens a session of it in the dev shell's headless
Chromium, and runs the script's default export with the Playwright page, the session
(`tests/explore-page/tests/session.ts`, which moves the round as the agent and the pane
would) and `shot(name)`, a full-page screenshot once the page has drawn. It prints what
the script prints, then the path of each screenshot, under a new folder of the system
temporary directory. Keep the script outside the repository, in a scratch folder: it is a
throwaway, and the run stores nothing in the repository. The script imports only types:
the packages resolve from `tests/explore-page`, not from the script's folder. The header
of [`script.ts`](../../tests/explore-page/script.ts) shows a script and lists the
variables: `SCRIPT_DATA=rich` plays the gallery's long round, `SCRIPT_WIDTH=390` a
phone's width.

Use it to see what a change does to a state the gallery has no entry for, or to check a
behaviour of the client by hand before deciding whether it earns a journey in the
[e2e tests](explore-page-e2e.md).

## Screenshot gallery

```sh
nix develop --command make explore-gallery
```

This takes a full-page screenshot of every state of the page, at 1280 and 390
pixels wide, in the light and the dark theme, and writes them with a contact
sheet, `index.html`, which shows each state's screenshots side by side. It calls no model, needs
no network once the npm packages are installed, and is not part of `make check`. A
state that fails to reach its page stops the run with its name.

Each image is named `<state>-<width>-<theme>.png`, so two runs compare file by file.
The variables are listed at the top of
[`gallery.ts`](../../tests/explore-page/gallery/gallery.ts): `GALLERY_COMPARE`, an
earlier gallery's folder, makes the contact sheet show before and after for each image
that differs.

Two runs on the same code give the same files, on any machine: the fixed clocks and the time
zone make every time the page shows the same; an image differs only where the
browser draws differently. The comparison is byte for byte, so such a difference
also counts as a change. To add a state of the page, add one entry to
[`tests/explore-page/gallery/states.ts`](../../tests/explore-page/gallery/states.ts),
whose header says what an entry holds. Before an action the round no longer offers, hold
the page (`refused` in that file) so that it does not follow the round first. A state
that needs a new move of the round needs a control route of the server first, as
for an e2e test.
