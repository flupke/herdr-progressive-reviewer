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
every request, and an open page loads itself again when one of them changes, so a client or style
edit shows at once. A Rust edit needs a rebuild: stop the server and run the command
again; the open page reconnects by itself once the server is back. The token stays
`dev`, so the page opens again without a new address.

The server's options are listed at the top of
[`main.rs`](../../crates/review-explore-page-server/src/main.rs). `--data rich` serves a
round as long as a real one, for the gallery below; `make explore-page
EXPLORE_PAGE_ARGS='--data rich'` serves it.

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
