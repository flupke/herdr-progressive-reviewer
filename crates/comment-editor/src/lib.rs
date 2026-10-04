//! Inline multiline editor for review comments.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crossterm::event::KeyEvent;
use edtui::actions::{Execute, InsertChar, LineBreak, SwitchMode};
use edtui::{EditorEventHandler, EditorMode, EditorState, Lines};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ui_shortcuts::Key;
use ui_theme::Palette;

mod motion;
mod view;
use view::EditorViewport;

#[cfg(test)]
#[path = "wrapping.tests.rs"]
mod wrapping_tests;

pub use review_types::EditorKeymap;

/// An editor with its own undo history, Vim state, and scroll position.
pub struct CommentEditor {
    state: RefCell<EditorState>,
    handler: EditorEventHandler,
    /// The keymap `handler` implements; it follows `setting` before each key.
    keymap: EditorKeymap,
    setting: KeymapSetting,
    viewport: RefCell<EditorViewport>,
    /// The text stays on one line: Enter is left to the view, and no key or paste breaks a line.
    single_line: bool,
}

/// The keymap chosen for every editor that shares this setting.
///
/// Clones share one value, so switching the keymap in one editor switches all of them.
#[derive(Clone, Debug, Default)]
pub struct KeymapSetting(Rc<Cell<EditorKeymap>>);

impl KeymapSetting {
    pub fn new(keymap: EditorKeymap) -> Self {
        Self(Rc::new(Cell::new(keymap)))
    }

    pub fn get(&self) -> EditorKeymap {
        self.0.get()
    }

    pub fn set(&self, keymap: EditorKeymap) {
        self.0.set(keymap);
    }
}

fn keymap_handler(keymap: EditorKeymap) -> EditorEventHandler {
    match keymap {
        EditorKeymap::Vim => EditorEventHandler::vim_mode(),
        EditorKeymap::Regular => EditorEventHandler::emacs_mode(),
    }
}

impl CommentEditor {
    pub fn new(text: &str, setting: &KeymapSetting) -> Self {
        let text = clean_text(text);
        let mut state = EditorState::new(Lines::from(text.as_str()));
        SwitchMode(EditorMode::Insert).execute(&mut state);
        let keymap = setting.get();
        Self {
            state: RefCell::new(state),
            handler: keymap_handler(keymap),
            keymap,
            setting: setting.clone(),
            viewport: RefCell::default(),
            single_line: false,
        }
    }

    /// An editor for a one-line value, such as a setting, with the cursor after `text`. Enter
    /// is left to the view that saves the value, and no key or paste breaks the line.
    pub fn single_line(text: &str, setting: &KeymapSetting) -> Self {
        let mut editor = Self::new(&text.replace(['\r', '\n'], " "), setting);
        editor.single_line = true;
        let state = editor.state.get_mut();
        state.cursor = edtui::Index2::new(0, text.chars().count());
        editor
    }

    pub fn saved_state(&self) -> review_types::TextEditorState {
        let state = self.state.borrow();
        review_types::TextEditorState {
            text: self.text(),
            row: state.cursor.row,
            column: state.cursor.col,
            scroll: state.viewport_offset().1,
            normal: state.mode == EditorMode::Normal,
        }
    }

    pub fn restore(saved: &review_types::TextEditorState, setting: &KeymapSetting) -> Self {
        let mut editor = Self::new(&saved.text, setting);
        let state = editor.state.get_mut();
        let lines: Vec<_> = saved.text.split('\n').collect();
        let row = saved.row.min(lines.len().saturating_sub(1));
        let column = saved.column.min(lines[row].chars().count());
        state.cursor = edtui::Index2::new(row, column);
        if editor.keymap == EditorKeymap::Vim && saved.normal {
            SwitchMode(EditorMode::Normal).execute(state);
        }
        state.set_viewport_offset(0, saved.scroll.min(row));
        editor
    }

    pub fn text(&self) -> String {
        String::from(self.state.borrow().lines.clone())
    }

    fn mode(&self) -> &'static str {
        match self.state.borrow().mode {
            EditorMode::Normal => "Normal",
            EditorMode::Insert => "Insert",
            EditorMode::Visual => "Visual",
            EditorMode::Search => "Search",
        }
    }

    pub fn input(&mut self, key: Key) {
        self.follow_setting();
        if key == Key::EditorMode {
            self.toggle_keymap();
        } else if self.single_line && key == Key::Enter {
        } else if let Some(event) = editor_key(key) {
            let state = self.state.get_mut();
            let before = self.single_line.then(|| state.clone());
            let motion = motion::VisualMotion::resolve(event, state.mode, &self.handler);
            let cursor = state.cursor;
            self.handler.on_key_event(event, state);
            if let Some(before) = before
                && state.lines.len() > 1
            {
                *state = before;
            }
            let viewport = self.viewport.get_mut();
            if let Some(motion) = motion {
                viewport.move_cursor(state, cursor, motion);
            } else {
                viewport.reset_motion();
            }
        }
    }

    fn toggle_keymap(&mut self) {
        let keymap = match self.keymap {
            EditorKeymap::Vim => EditorKeymap::Regular,
            EditorKeymap::Regular => EditorKeymap::Vim,
        };
        self.switch_keymap(keymap, true);
        self.setting.set(keymap);
    }

    /// Adopt a keymap another editor switched to, keeping this editor inserting.
    fn follow_setting(&mut self) {
        let keymap = self.setting.get();
        if keymap != self.keymap {
            self.switch_keymap(keymap, false);
        }
    }

    /// Change keymaps, entering Vim Normal mode only when `vim_normal` asks for it.
    fn switch_keymap(&mut self, keymap: EditorKeymap, vim_normal: bool) {
        let state = self.state.get_mut();
        let cursor = state.cursor;
        // Finish the current insert/search/selection and discard pending Vim keys.
        self.handler = EditorEventHandler::vim_mode();
        self.handler.on_key_event(Key::Escape.to_terminal(), state);
        if keymap == EditorKeymap::Regular || !vim_normal {
            SwitchMode(EditorMode::Insert).execute(state);
            state.cursor = cursor;
        }
        self.keymap = keymap;
        self.handler = keymap_handler(keymap);
    }

    pub fn paste(&mut self, text: &str) {
        self.follow_setting();
        let mut text = clean_text(text);
        if self.single_line {
            text = text.replace('\n', " ");
        }
        let state = self.state.get_mut();
        if state.mode != EditorMode::Insert {
            self.handler.on_paste_event(text, state);
            return;
        }
        // Edtui pastes after the cursor (Vim p), including in Insert mode.
        // Its public insertion actions preserve the actual insert position.
        let cursor = state.cursor;
        SwitchMode(EditorMode::Normal).execute(state);
        SwitchMode(EditorMode::Insert).execute(state);
        state.cursor = cursor;
        for character in text.replace("\r\n", "\n").chars() {
            if character == '\n' {
                LineBreak(1).execute(state);
            } else {
                InsertChar(character).execute(state);
            }
        }
    }

    /// Render an independent editor viewport suitable for insertion into a diff. An unfocused
    /// editor keeps its text and scroll position but draws no cursor.
    pub fn render(&self, area: Rect, buffer: &mut Buffer, palette: Palette, focused: bool) {
        if area.is_empty() {
            return;
        }
        self.viewport.borrow_mut().render(
            &mut self.state.borrow_mut(),
            area,
            buffer,
            palette,
            focused,
        );
    }

    /// A bottom border carrying the editing mode on the left and the keymap hint on the right.
    ///
    /// `width` counts the border cells between the caller's corners. Labels that do not fit
    /// are dropped, hint first.
    pub fn status_border(&self, width: u16, border: Style, palette: Palette) -> Line<'static> {
        let mode = if self.keymap == self.setting.get() {
            self.mode()
        } else {
            // follow_setting switches to Insert before the next key.
            "Insert"
        };
        let (mode, hint) = match self.setting.get() {
            EditorKeymap::Vim => (
                format!("Vim · {}", mode.to_uppercase()),
                "F2 · Regular editing",
            ),
            EditorKeymap::Regular => ("Regular editing".to_owned(), "F2 · Vim editing"),
        };
        let mut labels = vec![
            Span::styled(format!(" {mode} "), Style::default().fg(palette.warning)),
            Span::styled(format!(" {hint} "), Style::default().fg(palette.dim)),
        ];
        let width = usize::from(width);
        let labels_width = |labels: &[Span<'_>]| labels.iter().map(Span::width).sum::<usize>();
        while !labels.is_empty() && labels_width(&labels) + labels.len() + 1 > width {
            labels.pop();
        }
        let rule = |count: usize| Span::styled("─".repeat(count), border);
        if labels.is_empty() {
            return Line::from(rule(width));
        }
        // The first gap after the mode takes the spare width, pushing the hint right.
        let spare = width - labels_width(&labels) - labels.len() - 1;
        let mut spans = vec![rule(1)];
        for (index, label) in labels.into_iter().enumerate() {
            spans.push(label);
            spans.push(rule(1 + if index == 0 { spare } else { 0 }));
        }
        Line::from(spans)
    }
}

fn clean_text(text: &str) -> String {
    text.replace("\r\n", "\n")
        .chars()
        .filter(|character| !character.is_control() || matches!(character, '\n' | '\t'))
        .collect()
}

fn editor_key(key: Key) -> Option<KeyEvent> {
    (!matches!(key, Key::Control('s') | Key::ControlEnter)).then(|| key.to_terminal())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_s_does_not_search_move_or_edit_in_either_keymap() {
        for keymap in [EditorKeymap::Vim, EditorKeymap::Regular] {
            let mut editor = CommentEditor::new("abc abc", &KeymapSetting::new(keymap));
            editor.input(Key::Right);
            let cursor = editor.state.borrow().cursor;
            let mode = editor.mode();
            editor.input(Key::Control('s'));
            assert_eq!(editor.mode(), mode);
            assert_eq!(editor.state.borrow().cursor, cursor);
            assert_eq!(editor.text(), "abc abc");
            editor.input(Key::Char('x'));
            assert_eq!(editor.text(), "axbc abc");
        }
    }

    #[test]
    fn a_single_line_editor_starts_after_its_text_and_never_breaks_the_line() {
        for keymap in [EditorKeymap::Vim, EditorKeymap::Regular] {
            let mut editor = CommentEditor::single_line("8790", &KeymapSetting::new(keymap));
            editor.input(Key::Char('1'));
            assert_eq!(editor.text(), "87901");
            editor.input(Key::Enter);
            editor.paste("2\n3");
            assert_eq!(editor.text(), "879012 3");
            editor.input(Key::Escape);
            editor.input(Key::Char('o'));
            editor.input(Key::Char('4'));
            assert!(
                !editor.text().contains('\n'),
                "{keymap:?}: {:?}",
                editor.text()
            );
        }
    }

    #[test]
    fn ctrl_enter_is_left_to_the_view_that_submits_the_draft() {
        for keymap in [EditorKeymap::Vim, EditorKeymap::Regular] {
            let mut editor = CommentEditor::new("abc", &KeymapSetting::new(keymap));
            editor.input(Key::Right);
            let cursor = editor.state.borrow().cursor;
            let mode = editor.mode();
            editor.input(Key::ControlEnter);
            assert_eq!(editor.state.borrow().cursor, cursor);
            assert_eq!(editor.mode(), mode);
            assert_eq!(editor.text(), "abc");
        }
    }

    #[test]
    fn switching_keymaps_preserves_the_draft_and_changes_escape_and_typing() {
        let mut editor = CommentEditor::new("abc", &KeymapSetting::new(EditorKeymap::Vim));
        editor.input(Key::Right);
        editor.input(Key::EditorMode);
        assert_eq!(editor.keymap, EditorKeymap::Regular);
        editor.input(Key::Escape);
        assert_eq!(editor.mode(), "Insert");
        editor.input(Key::Char('x'));
        assert_eq!(editor.text(), "axbc");
        editor.input(Key::Backspace);
        assert_eq!(editor.text(), "abc");

        editor.input(Key::EditorMode);
        assert_eq!(editor.keymap, EditorKeymap::Vim);
        assert_eq!(editor.mode(), "Normal");
        editor.input(Key::First);
        editor.input(Key::Char('x'));
        assert_eq!(editor.text(), "bc");
        editor.input(Key::Char('u'));
        assert_eq!(editor.text(), "abc");
    }

    #[test]
    fn switching_one_editor_switches_every_editor_sharing_the_setting() {
        let setting = KeymapSetting::new(EditorKeymap::Vim);
        let mut first = CommentEditor::new("", &setting);
        let mut second = CommentEditor::new("ab", &setting);
        let palette = ui_theme::Theme::default().palette;
        first.input(Key::EditorMode);
        assert_eq!(setting.get(), EditorKeymap::Regular);
        assert!(
            second
                .status_border(40, Style::default(), palette)
                .to_string()
                .starts_with("─ Regular editing")
        );
        second.input(Key::Escape);
        second.input(Key::Char('x'));
        assert_eq!(second.text(), "xab");

        second.input(Key::EditorMode);
        let later = CommentEditor::new("", &setting);
        assert_eq!(later.keymap, EditorKeymap::Vim);
        first.input(Key::Char('y'));
        assert_eq!(first.text(), "y", "a followed editor keeps inserting");
    }

    #[test]
    fn switching_keymaps_cancels_pending_vim_commands_and_search() {
        let mut editor = CommentEditor::new("abc", &KeymapSetting::new(EditorKeymap::Vim));
        editor.input(Key::Escape);
        editor.input(Key::Char('d'));
        editor.input(Key::EditorMode);
        editor.input(Key::Char('w'));
        assert_eq!(editor.text(), "wabc");
        editor.input(Key::EditorMode);
        editor.input(Key::Char('/'));
        editor.input(Key::Char('b'));
        assert_eq!(editor.mode(), "Search");
        editor.input(Key::EditorMode);
        editor.input(Key::First);
        editor.input(Key::Char('i'));
        assert_eq!(editor.text(), "iwabc");
    }

    #[test]
    fn status_border_highlights_the_active_keymap_and_shows_the_vim_state() {
        let mut editor = CommentEditor::new("draft", &KeymapSetting::new(EditorKeymap::Vim));
        let palette = ui_theme::Theme::default().palette;
        let border = Style::default().fg(palette.focus);
        let text = |line: &Line<'_>| line.to_string();
        let line = editor.status_border(40, border, palette);
        assert_eq!(line.width(), 40);
        assert!(text(&line).starts_with("─ Vim · INSERT ─"));
        assert!(text(&line).ends_with("─ F2 · Regular editing ─"));
        assert_eq!(line.spans[1].style.fg, Some(palette.warning));
        assert_eq!(line.spans[3].style.fg, Some(palette.dim));
        assert_eq!(line.spans[0].style, border);

        editor.input(Key::Escape);
        assert!(text(&editor.status_border(40, border, palette)).starts_with("─ Vim · NORMAL"));
        editor.input(Key::EditorMode);
        let line = editor.status_border(40, border, palette);
        assert!(text(&line).starts_with("─ Regular editing ─"));
        assert!(text(&line).ends_with("─ F2 · Vim editing ─"));
        assert_eq!(editor.text(), "draft");
    }

    #[test]
    fn narrow_status_borders_drop_the_hint_then_the_mode() {
        let editor = CommentEditor::new("", &KeymapSetting::new(EditorKeymap::Regular));
        let palette = ui_theme::Theme::default().palette;
        for (width, expected) in [
            (20, "─ Regular editing ──"),
            (19, "─ Regular editing ─"),
            (18, "──────────────────"),
        ] {
            let line = editor.status_border(width, Style::default(), palette);
            assert_eq!(line.to_string(), expected);
        }
    }

    #[test]
    fn a_single_editor_row_keeps_the_draft_and_cursor_visible() {
        let palette = ui_theme::Theme::default().palette;
        let area = Rect::new(0, 0, 16, 1);
        for keymap in [EditorKeymap::Vim, EditorKeymap::Regular] {
            let mut editor = CommentEditor::new("draft", &KeymapSetting::new(keymap));
            editor.input(Key::Last);
            editor.input(Key::Char('x'));
            let mut buffer = Buffer::empty(area);
            editor.render(area, &mut buffer, palette, true);
            let visible = buffer
                .content()
                .iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>();
            assert!(visible.starts_with("draftx"));
            assert_eq!(buffer[(6, 0)].bg, palette.focus);
            assert!(!visible.contains("Vim ·"));
        }
    }

    #[test]
    fn change_word_preserves_spacing_and_repeats_the_replacement() {
        let mut editor =
            CommentEditor::new("one two three", &KeymapSetting::new(EditorKeymap::Vim));
        editor.input(Key::Escape);
        for character in "cwX".chars() {
            editor.input(Key::Char(character));
        }
        assert_eq!(editor.mode(), "Insert");
        assert_eq!(editor.text(), "X two three");
        editor.input(Key::Escape);
        for character in "w.".chars() {
            editor.input(Key::Char(character));
        }
        assert_eq!(editor.text(), "X X three");
        assert_eq!(editor.mode(), "Normal");
    }

    #[test]
    fn delete_word_can_be_repeated_and_undone() {
        let mut editor =
            CommentEditor::new("one two three", &KeymapSetting::new(EditorKeymap::Vim));
        editor.input(Key::Escape);
        for character in "dw".chars() {
            editor.input(Key::Char(character));
        }
        assert_eq!(editor.text(), "two three");
        editor.input(Key::Char('.'));
        assert_eq!(editor.text(), "three");
        editor.input(Key::Char('u'));
        assert_eq!(editor.text(), "two three");
    }

    #[test]
    fn editing_and_vim_undo_preserve_multiline_unicode_text() {
        let mut editor = CommentEditor::new("", &KeymapSetting::new(EditorKeymap::Vim));
        for key in [Key::Char('é'), Key::Enter, Key::Char('界')] {
            editor.input(key);
        }
        assert_eq!(editor.text(), "é\n界");
        editor.input(Key::Escape);
        assert_eq!(editor.mode(), "Normal");
        editor.input(Key::Char('u'));
        assert_ne!(editor.text(), "é\n界");
    }

    #[test]
    fn bracketed_paste_inserts_at_the_insert_cursor() {
        let mut editor = CommentEditor::new("ac", &KeymapSetting::new(EditorKeymap::Vim));
        editor.input(Key::Right);
        editor.paste("b\nsecond");
        assert_eq!(editor.text(), "ab\nsecondc");
    }

    #[test]
    fn paste_at_start_and_end_handles_leading_newlines() {
        let mut editor = CommentEditor::new("middle", &KeymapSetting::new(EditorKeymap::Vim));
        editor.paste("\nfirst\n");
        assert_eq!(editor.text(), "\nfirst\nmiddle");
        editor.input(Key::Last);
        editor.paste("終");
        assert_eq!(editor.text(), "\nfirst\nmiddle終");
    }
}
