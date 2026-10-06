# Parallel workspaces

How to run a gate or an agent in a second jj workspace: the dev shell, disk space, and keeping the work visible.

`/tmp` may be a RAM disk, and a full build needs about 16 GB. Put a second jj
workspace or checkout on disk, or point `CARGO_TARGET_DIR` at the main
checkout's `target/`, before running a gate in it.

A jj workspace has no `.git`, so `nix develop` there copies the whole directory,
`target/` included, into the Nix store. Enter the dev shell through
`scripts/dev-shell`, which hands Nix a copy of `flake.nix` and `flake.lock`
alone, a changed flake included:

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

The Herdr integration tests copy their test binary, about 400 MB, into a
private directory under `/tmp` for each test. When several gates run at once, a
test that talks to the Herdr socket can fail with `WouldBlock`, and pass when
run alone.
