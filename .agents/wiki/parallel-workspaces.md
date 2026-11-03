# Parallel workspaces

How to run a gate or an agent in a second jj workspace: the dev shell, disk space, and keeping the work visible.

`/tmp` may be a RAM disk, and a full build needs about 16 GB. Put a second jj
workspace or checkout on disk, or point `CARGO_TARGET_DIR` at the main
checkout's `target/`, before running a gate in it.

The flake lives in `nix/`, which `scripts/dev-shell` and `.envrc` load as a
`path:` URL: Nix copies that directory alone into the store. A bare `nix develop`
at the root finds no flake; `nix develop .` or a flake at the root would copy the
whole jj workspace, which has no `.git`, `target/` included, at each run (tens of
GB each). Enter the dev shell through `scripts/dev-shell`:

```sh
scripts/dev-shell make check-changed
```

Work in another workspace must stay visible to the user, who follows it from
the main checkout with `jj log` and `jj diff -r <bookmark>`:

- The supervisor names each agent's workspace and bookmark when it starts it.
- The agent describes its change at the start, with a provisional subject.
- The agent runs `jj st` after each batch of edits: jj records a workspace's
  files only when a jj command runs there.
- The agent closes each slice that works with `jj new`, so progress shows as
  described commits.

Each Herdr test server keeps its sockets, state and the link to the stand-in
agent's program in a private directory under `TMPDIR`; the server's shell removes
it once the server stops, even when the test process was killed. When several
gates run at once, a test that talks to the Herdr socket can fail with
`WouldBlock`, and pass when run alone.
