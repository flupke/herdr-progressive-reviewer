# Progressive reviewer

This Herdr plugin reviews the current jj change one file at a time. It stores
review marks on disk and inserts selected diff text into the last focused agent
pane without submission.

It requires Herdr 0.7.5 or later and jj 0.43.0 or later.

## Development install

Build both programs and link this directory:

```sh
cargo build --release --locked --bins
mkdir -p bin
cp target/release/pr-app target/release/pr-control bin/
herdr plugin link --enabled .
```

The Herdr action list then contains `open`, `close`, and `toggle`.

## Use

Run the `Open progressive reviewer` action from a jj workspace. Use `Tab` to
change focus, `j` and `k` to move, `Space` to change the file review state,
and `v` plus `Enter` to insert selected diff lines. The plugin does not submit
the agent prompt.

## Release package

Run:

```sh
scripts/package.sh
```

The script builds native release programs and writes a `.tar.gz` file to
`dist/`. Extract that file and link the extracted directory with
`herdr plugin link --enabled PATH`.

## Manual release checks

Before release, test each supported agent TUI:

1. Open, reopen, toggle, and close the review pane.
2. Mark a file, edit it, and confirm that its marker changes.
3. Select diff lines and insert them into the last focused agent.
4. Confirm that insertion does not submit the prompt.
5. Confirm that you can add a comment after the inserted diff.
6. Repeat in pure jj and colocated jj/Git workspaces.
7. Confirm that a Git-only directory is rejected.
