# Explore page: client

The page's JavaScript client: how it is served, how to add a screen or a region, where its types come from and how they are checked.

The client is plain JavaScript ES modules in
[`crates/review-explore-page/assets/client`](../../crates/review-explore-page/assets/client),
committed as they are: there is no build step, and the browser loads each module from
`/assets/client/<name>`. Every module must be listed in `ASSETS` in `src/files.rs`, which
builds it into the binary and serves it with the script content type browsers require
for modules. The page's content security policy allows scripts from the page itself
only, with no `unsafe` value. `dom.js` holds the rendering rules, the one place to read
before changing a screen.

To add a screen or a region: give the view the data it needs (a field of `PageView` or of
the type of its screen, in `src/view.rs`, built from the round's snapshot), run
`make explore-types`, write the module that builds the region from that data, add its
region to `Page` in `page.js`, and list the module in `ASSETS`. An action the region
offers is a form with a `data-method`, an entry in `CALLS` in `actions.js`, a variant of
`Call` in `src/rpc.rs`, and its check in `src/actions.rs`, which hands it to the owner as a
`PageCommand`. A new field of the view is one line on each side.

The client's types come from the Rust types of the socket's messages: ts-rs generates
`assets/client/types.ts` from them (`src/typescript.rs`), and each module names them in
its JSDoc (`/** @import { PageView } from "./types.ts" */`). The file is committed. A
Rust test fails once it no longer matches the Rust types (`make explore-types` writes it
again), and `make lint` runs the Nix shell's `tsc` over the client
(`tests/explore-page/tsconfig.client.json`), which fails on a field or a variant that one
side no longer has. `tsc` emits
nothing: the browser loads the modules as they are.

The socket is tested in Rust (`src/socket.tests.rs`), and what the reviewer does on the page
with the [e2e tests](explore-page-e2e.md); the client has no unit tests.
