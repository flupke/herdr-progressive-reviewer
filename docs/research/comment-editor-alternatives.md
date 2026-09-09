# Broader Vim editing for comments

Checked 2026-09-10 against published crate archives and crates.io metadata.
At the start of this comparison, the reviewer used Rust 1.89, Ratatui 0.29,
Crossterm 0.28, and Edtui 0.9.9.
This revisits [the original editor choice](comment-editor.md) after missing
`cw`/`dw` exposed the limitations of that compatible Edtui release.

## Decision

Upgrade to Edtui 0.11.7 for now, as requested, to supply `cw` and `dw`.
The workspace moves to Ratatui 0.30 and Crossterm 0.29, with ansi-to-tui 8.
ratatui-markdown 0.3.6 still requires Ratatui 0.29, so hover rendering retains
that dependency behind a compatibility adapter preserving colors, modifiers,
alignment, and the existing syntax-highlighting hook. The editor and the rest
of the UI share the new Ratatui types.

This does not establish Codex parity: Edtui still lacks general operator
composition, around text objects, and replace mode. Its dot recording also
does not capture Backspace or newline edits made during insertion; its tests
establish repetition of simple inserted text only.
[Bindings and tests][edtkeys], [insertion recording][edtinsert],
[repeat implementation][edtactions].

## Alternative considered

**For broader everyday Vim editing, evaluate `modalkit = "=0.0.24"` with
`modalkit-ratatui = "=0.0.24"`.** Its operator/motion model is closer to the
requested behavior. Both
manifests declare Rust 1.74 and Crossterm `^0.28.1`; the widget uses Ratatui
`^0.29.0`. Its text-object limitations below are material tradeoffs, not full
Vim compatibility. [Engine manifest][mkmanifest], [widget manifest][mrmanifest].

Edtui 0.11.7 supplies `cw`, `dw`, and dot repeat for simple insertions, but still omits
ordinary combinations such as `db` and `yw`, as well as counts. Upgrading it
would address the named commands while leaving the broader limitation. It also
requires migration to the Ratatui 0.30 ecosystem. These are source-based
recommendations from the initial comparison; the Edtui upgrade was selected
afterwards.
[Bindings and tests][edtkeys], [manifest][edtmanifest].

The adapter would keep a `Store`, `KeyManager` using `default_vim_keys`, and
`TextBoxState`; route generated editor, repeat, macro, scroll, and jump actions
to their library handlers; and render the supplied `TextBox`. The upstream
single-textbox example already demonstrates that arrangement. The reviewer would own
save/cancel policy and paste integration, while the library owns command
interpretation, buffer changes, undo, and viewport tracking. Search command-bar
actions need explicit integration; the minimal example ignores them.
[Example][mkexample], [textbox implementation][textbox].

## Editing behavior verified in published source

| Candidate | Verified behavior | Limits relevant to this choice |
| --- | --- | --- |
| Edtui 0.11.7 | Explicit bindings for `cw`, `dw`, `diw`, WORD variants, inner quotes/brackets, `cf`/`ct`/`df`/`dt`, undo/redo, and dot repeat. Included tests exercise `dw.`, repeated `cw`/`ciw` including inserted text, insert sessions, and `df.`. | Still a fixed sequence-to-action table: no numeric count parser or general operator-pending grammar. `3w`, `d2w`, `db`, and `yw` are not supplied by that table. Tests were inspected, not run. [Bindings and tests][edtkeys]. |
| Modalkit 0.0.24 | Operator-pending `c`/`d`/`y`, word/WORD and character-find motions, line motions, counts, word and bracket/quote objects. Upstream key tests cover `dw`, `cw`, `caw`, `2d2w` count multiplication, and dot repeat including insertion and count override; buffer tests cover delete/change/yank and undo/redo. | `aw` and `iw` (also `aW`/`iW`) generate identical ranges, so Vim's whitespace distinction is missing. Sentence, paragraph, and XML objects are mapped but their range implementation returns `None`. `gv`, `gi`, `gR`, and others are explicitly unmapped. These gaps remain in 0.0.26. [Key tests][mkkeys], [edit tests][mkedit], [undo tests][mkbuffer], [range implementation][mkrope], [0.0.26 ranges][mknewrope]. |

Modalkit's quote/bracket range tests include inner/outer handling; its text
execution code distinguishes those ranges, unlike the word-object distinction.
This supports calling it more complete for ordinary editing, not full Vim.
[Range tests and implementation][mkrope].

## Upgrade and dependency cost

| Version | Ratatui dependency | Crossterm | Declared Rust minimum |
| --- | --- | --- | --- |
| Edtui 0.11.7, released 2026-08-16 | `ratatui-core ^0.1`, `ratatui-widgets ^0.3.0` | `^0.29` | Edtui does not declare one; current compatible core/widgets releases declare 1.88. |
| Modalkit pair 0.0.24, engine released 2025-08-16 | Widget uses `^0.29.0` | `^0.28.1` | 1.74 |
| Modalkit pair 0.0.25 | Widget uses `^0.29.0` | `^0.29` | 1.75 |
| Modalkit pair 0.0.26, engine released 2026-09-02 | Widget uses `^0.30.0` | `^0.29` | 1.88 |

Sources: [Edtui manifest][edtmanifest], [Edtui release metadata][edtdate],
[Modalkit index][mkindex], [widget index][mrindex],
[0.0.24 release metadata][mkdate], [0.0.26 release metadata][mknewdate],
[Ratatui core index][coreindex], [widgets index][widgetsindex].
These declared minimums do not prove that the full dependency resolution builds
with Rust 1.89; that remains an integration check.

**An Edtui upgrade is not a direct widget replacement.** Its `EditorView`
implements the split `ratatui-core` widget trait and consumes that crate's
`Buffer`, `Rect`, and styles. The reviewer's Ratatui 0.29 types are different types.
Using the new widget directly therefore requires migration to the Ratatui 0.30
ecosystem. A local compatibility bridge could instead render into a new buffer
and translate cells/styles into the old buffer, plus translate events; that is
an untested alternative with rendering maintenance, not a small event-only
adapter. Modalkit 0.0.25 would need an event conversion but retains compatible
Ratatui widget types; 0.0.24 avoids both boundaries.
[Edtui view][edtview], [theme][edttheme], [manifests/indexes above][mrindex].

## Other options checked

`vimltui` 0.2.11 advertises composable operators, counts, text objects, undo,
and dot repeat, but requires Ratatui 0.30/Crossterm 0.29. Its published source
maps `cw` to the same word-forward range as `dw`; code inspection therefore
indicates it removes the following whitespace instead of preserving it as Vim
does on a word. No `#[test]` functions were found in its published source.
It offers no clear advantage for this task. [Manifest][vimmanifest],
[input][viminput], [operator execution][vimops], [motions][vimmotions].

Codex 0.154.0 supplies its own crate-private `TextArea` and local Vim modules,
coupled to Codex types; its tests cover `dw.`, `cwX<Esc>w.`, `cc`, and `c$`.
Its editing experience is a useful target, but it does not supply an editor
dependency the reviewer can substitute directly. [Pinned textarea source][codextext],
[command tests][codextests], [Codex TUI manifest][codexmanifest].

The initial comparison inspected published source without running its tests.
The subsequent upgrade adds reviewer regression tests for `cw`, `dw`, dot
repeat of those commands, and undo, alongside the existing paste tests.

[mkmanifest]: https://docs.rs/crate/modalkit/0.0.24/source/Cargo.toml
[mrmanifest]: https://docs.rs/crate/modalkit-ratatui/0.0.24/source/Cargo.toml
[mkexample]: https://docs.rs/modalkit-ratatui/0.0.24/modalkit_ratatui/
[textbox]: https://docs.rs/crate/modalkit-ratatui/0.0.24/source/src/textbox.rs
[edtkeys]: https://docs.rs/crate/edtui/0.11.7/source/src/events/key.rs
[edtinsert]: https://docs.rs/crate/edtui/0.11.7/source/src/actions/insert.rs
[edtactions]: https://docs.rs/crate/edtui/0.11.7/source/src/actions.rs
[mkkeys]: https://docs.rs/crate/modalkit/0.0.24/source/src/env/vim/keybindings.rs
[mkedit]: https://docs.rs/crate/modalkit/0.0.24/source/src/editing/buffer/edit.rs
[mkbuffer]: https://docs.rs/crate/modalkit/0.0.24/source/src/editing/buffer/mod.rs
[mkrope]: https://docs.rs/crate/modalkit/0.0.24/source/src/editing/rope/mod.rs
[mknewrope]: https://docs.rs/crate/modalkit/0.0.26/source/src/editing/rope/mod.rs
[edtmanifest]: https://docs.rs/crate/edtui/0.11.7/source/Cargo.toml.orig
[edtdate]: https://crates.io/api/v1/crates/edtui/0.11.7
[mkindex]: https://index.crates.io/mo/da/modalkit
[mrindex]: https://index.crates.io/mo/da/modalkit-ratatui
[mkdate]: https://crates.io/api/v1/crates/modalkit/0.0.24
[mknewdate]: https://crates.io/api/v1/crates/modalkit/0.0.26
[coreindex]: https://index.crates.io/ra/ta/ratatui-core
[widgetsindex]: https://index.crates.io/ra/ta/ratatui-widgets
[edtview]: https://docs.rs/crate/edtui/0.11.7/source/src/view.rs
[edttheme]: https://docs.rs/crate/edtui/0.11.7/source/src/view/theme.rs
[vimmanifest]: https://docs.rs/crate/vimltui/0.2.11/source/Cargo.toml.orig
[viminput]: https://docs.rs/crate/vimltui/0.2.11/source/src/editor/input.rs
[vimops]: https://docs.rs/crate/vimltui/0.2.11/source/src/editor/operators.rs
[vimmotions]: https://docs.rs/crate/vimltui/0.2.11/source/src/editor/motions.rs
[codextext]: https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/tui/src/bottom_pane/textarea.rs
[codextests]: https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/tui/src/bottom_pane/textarea/vim_commands_tests.rs
[codexmanifest]: https://github.com/openai/codex/blob/rust-v0.154.0/codex-rs/tui/Cargo.toml
