# Inline comment editor

Checked 2026-09-10 against Herdr's Ratatui 0.29, Crossterm 0.28, and Rust 1.89.
Dependency claims below were checked against published crate manifests and the
crates.io index, rather than assuming that current documentation describes older
releases.

## Recommendation

Use `edtui = { version = "=0.9.9", default-features = false }` for the inline
multiline editor. It supplies Vim modes, undo/redo, cursor movement, editable
rendering, wrapping, and automatic viewport tracking without implementing a Vim
state machine locally. It is the last published Edtui release using Ratatui
0.29; 0.10.0 changes to 0.30. Edtui remains developed, although this compatible
release is older than its current 0.11 series.
[Published versions](https://index.crates.io/ed/tu/edtui),
[0.9.9 package source](https://docs.rs/crate/edtui/0.9.9/source/)

Disable defaults to omit `arboard`, mouse support, and syntax highlighting
(`syntect`/`once_cell`). Internal yank/paste and terminal bracketed paste still
work. The remaining direct dependencies are Ratatui 0.29, `edtui-jagged` 0.1,
`enum_dispatch` 0.3.12, and `unicode-width` 0.2.0. Edtui enables Ratatui's
`unstable` feature and uses Ratatui's Crossterm re-export, matching the existing
0.28 event types. Its manifest is edition 2021 and does not declare an MSRV;
Rust 1.89 compatibility must therefore be confirmed by the actual build.
[Manifest](https://docs.rs/crate/edtui/0.9.9/source/Cargo.toml.orig),
[Clipboard](https://docs.rs/crate/edtui/0.9.9/source/src/clipboard.rs)

## Alternatives

| Candidate | Compatibility and tradeoff |
| --- | --- |
| `tui-textarea = "=0.7.0"` | Smallest conventional textarea. Ratatui 0.29/Crossterm 0.28; built-in editing, selection, undo, and scrolling. Vim support is an example state machine, not a reusable built-in mode. Published release dates back to October 2024. |
| `ratatui-textarea` 0.9.2 | Ratatui-maintained continuation, but uses the split `ratatui-core`/`ratatui-widgets` 0.30 ecosystem. Not compatible with this workspace's widget types. |
| `tui-textarea-2` 0.13.2 | Maintained fork; likewise uses split Ratatui crates and Crossterm 0.29. No published compatible 0.29 release in its index. |
| `rat-text = "=2.8.0"` | Compatible with Ratatui 0.29/Crossterm 0.28. Rope-backed text, grapheme-aware movement, wrapping, scrolling, and undo. No documented built-in Vim mode found. Substantially broader dependencies, including several rat-salsa crates, locales, and formatting support. Current 3.x has moved to split Ratatui crates. |

Sources: [tui-textarea release](https://docs.rs/crate/tui-textarea/0.7.0),
[ratatui-textarea manifest](https://docs.rs/crate/ratatui-textarea/0.9.2/source/Cargo.toml),
[maintained fork](https://github.com/srothgan/tui-textarea),
[fork version index](https://index.crates.io/tu/i-/tui-textarea-2),
[rat-text manifest](https://docs.rs/crate/rat-text/2.8.0/source/Cargo.toml.orig),
[rat-text documentation](https://docs.rs/crate/rat-text/2.8.0/source/readme.md),
[rat-text version index](https://index.crates.io/ra/t-/rat-text).

## Edtui 0.9.9 integration

Keep `EditorState` and `EditorEventHandler` together in the draft-comment type:

```rust
let mut state = EditorState::new(Lines::from(existing_comment));
state.mode = EditorMode::Insert;
let mut events = EditorEventHandler::default();

events.on_key_event(key, &mut state);
events.on_paste_event(text, &mut state);

frame.render_widget(
    EditorView::new(&mut state)
        .wrap(true)
        .theme(EditorTheme::default().hide_status_line()),
    comment_area,
);

let comment = String::from(state.lines.clone());
```

The renderer updates its internal viewport as the cursor moves. Keep that state
between frames; render the editor itself instead of converting its text into a
separate paragraph. The theme also accepts a block and cursor/selection styles.
The handler accepts Crossterm key events directly or complete events through
`on_event`. Filter key-release events in the application first: Edtui's conversion
does not inspect `KeyEventKind`.
[State](https://docs.rs/crate/edtui/0.9.9/source/src/state.rs),
[View](https://docs.rs/crate/edtui/0.9.9/source/src/view.rs),
[Events](https://docs.rs/crate/edtui/0.9.9/source/src/events/mod.rs),
[Key conversion](https://docs.rs/crate/edtui/0.9.9/source/src/events/key.rs),
[Text conversion](https://docs.rs/crate/edtui-jagged/0.1.12/source/src/jagged/lines.rs)

Application policy: Enter inserts a newline; intercept a separate save shortcut
before forwarding input. Preserve Escape for Insert-to-Normal mode, then offer a
separate cancellation action. Show the current mode and save/cancel hints if
hiding Edtui's status line. These are integration recommendations, not crate
requirements.

Enable terminal bracketed paste on entry and disable it during cleanup. The
handler accepts pasted strings without the system-clipboard feature. Its paste
implementation uses the Vim paste action even in Insert mode and treats a
leading newline specially; verify paste at the start, middle, and end of text,
including multiline input. The buffer stores Unicode scalar values (`char`) and
uses display widths, so UTF-8 text is supported, but this is not a guarantee of
grapheme-cluster editing for combining marks or joined emoji. Test those cases
before promising grapheme-aware behavior.
[Paste handler](https://docs.rs/crate/edtui/0.9.9/source/src/events/paste.rs),
[Paste action](https://docs.rs/crate/edtui/0.9.9/source/src/actions/cpaste.rs),
[Library documentation](https://docs.rs/crate/edtui/0.9.9/source/src/lib.rs)

No workspace dependencies were changed and no tests were run for this research.
