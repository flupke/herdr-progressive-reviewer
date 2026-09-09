//! Inline multiline editor for review comments.

use std::cell::RefCell;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use edtui::actions::{Execute, InsertChar, LineBreak, SwitchMode};
use edtui::{EditorEventHandler, EditorMode, EditorState, EditorTheme, EditorView, Lines};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;
use ui_shortcuts::Key;
use ui_theme::Palette;

/// An editor with its own undo history, Vim state, and scroll position.
pub struct CommentEditor {
    state: RefCell<EditorState>,
    handler: EditorEventHandler,
    keymap: EditorKeymap,
}

/// Choose modal Vim commands or regular, always-inserting text input.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditorKeymap {
    #[default]
    Vim,
    Regular,
}

impl EditorKeymap {
    fn handler(self) -> EditorEventHandler {
        match self {
            Self::Vim => EditorEventHandler::vim_mode(),
            Self::Regular => EditorEventHandler::emacs_mode(),
        }
    }
}

impl CommentEditor {
    pub fn new(text: &str, keymap: EditorKeymap) -> Self {
        let text = clean_text(text);
        let mut state = EditorState::new(Lines::from(text.as_str()));
        SwitchMode(EditorMode::Insert).execute(&mut state);
        Self {
            state: RefCell::new(state),
            handler: keymap.handler(),
            keymap,
        }
    }

    pub fn text(&self) -> String {
        String::from(self.state.borrow().lines.clone())
    }

    pub fn mode(&self) -> &'static str {
        match self.state.borrow().mode {
            EditorMode::Normal => "Normal",
            EditorMode::Insert => "Insert",
            EditorMode::Visual => "Visual",
            EditorMode::Search => "Search",
        }
    }

    pub fn keymap(&self) -> EditorKeymap {
        self.keymap
    }

    pub fn input(&mut self, key: Key) {
        if key == Key::EditorMode {
            self.toggle_keymap();
        } else if let Some(event) = editor_key(key) {
            self.handler.on_key_event(event, self.state.get_mut());
        }
    }

    fn toggle_keymap(&mut self) {
        let state = self.state.get_mut();
        let cursor = state.cursor;
        // Finish the current insert/search/selection and discard pending Vim keys.
        self.handler = EditorEventHandler::vim_mode();
        self.handler
            .on_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), state);
        self.keymap = match self.keymap {
            EditorKeymap::Vim => {
                SwitchMode(EditorMode::Insert).execute(state);
                state.cursor = cursor;
                EditorKeymap::Regular
            }
            EditorKeymap::Regular => EditorKeymap::Vim,
        };
        self.handler = self.keymap.handler();
    }

    pub fn paste(&mut self, text: &str) {
        let text = clean_text(text);
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

    /// Render an independent editor viewport suitable for insertion into a diff.
    pub fn render(&self, area: Rect, buffer: &mut Buffer, palette: Palette) {
        if area.is_empty() {
            return;
        }
        let theme = EditorTheme::default()
            .base(Style::default().fg(palette.text))
            .cursor_style(Style::default().fg(palette.cursor).bg(palette.focus))
            .selection_style(Style::default().bg(palette.selection))
            .hide_status_line();
        let footer_height = u16::from(area.height > 1);
        EditorView::new(&mut self.state.borrow_mut())
            .theme(theme)
            .wrap(true)
            .render(
                Rect::new(area.x, area.y, area.width, area.height - footer_height),
                buffer,
            );
        if footer_height > 0 {
            self.status_line(area.width, palette)
                .render(Rect::new(area.x, area.bottom() - 1, area.width, 1), buffer);
        }
    }

    fn status_line(&self, width: u16, palette: Palette) -> Line<'static> {
        let active = Style::default().fg(palette.warning);
        let inactive = Style::default().fg(palette.dim);
        let (mode, hint) = match self.keymap {
            EditorKeymap::Vim => (
                format!("Vim · {}", self.mode().to_uppercase()),
                "F2 · Regular editing",
            ),
            EditorKeymap::Regular => ("Regular editing".to_owned(), "F2 · Vim editing"),
        };
        let mut line = Line::from(Span::styled(mode, active));
        let hint = Span::styled(hint, inactive);
        if line.width() + hint.width() < usize::from(width) {
            line.spans.push(Span::raw(
                " ".repeat(usize::from(width) - line.width() - hint.width()),
            ));
            line.spans.push(hint);
        }
        line
    }
}

fn clean_text(text: &str) -> String {
    text.replace("\r\n", "\n")
        .chars()
        .filter(|character| !character.is_control() || matches!(character, '\n' | '\t'))
        .collect()
}

fn editor_key(key: Key) -> Option<KeyEvent> {
    let (code, modifiers) = match key {
        Key::Control('s') => return None,
        Key::Char(character) => (KeyCode::Char(character), KeyModifiers::NONE),
        Key::Control(character) => (KeyCode::Char(character), KeyModifiers::CONTROL),
        Key::Alt(character) => (KeyCode::Char(character), KeyModifiers::ALT),
        _ => EDITOR_KEYS
            .iter()
            .find_map(|(candidate, code, modifiers)| {
                (*candidate == key).then_some((*code, *modifiers))
            })?,
    };
    Some(KeyEvent::new(code, modifiers))
}

const EDITOR_KEYS: &[(Key, KeyCode, KeyModifiers)] = &[
    (Key::Left, KeyCode::Left, KeyModifiers::NONE),
    (Key::Right, KeyCode::Right, KeyModifiers::NONE),
    (Key::Up, KeyCode::Up, KeyModifiers::NONE),
    (Key::Down, KeyCode::Down, KeyModifiers::NONE),
    (Key::First, KeyCode::Home, KeyModifiers::NONE),
    (Key::Last, KeyCode::End, KeyModifiers::NONE),
    (Key::PageUp, KeyCode::PageUp, KeyModifiers::NONE),
    (Key::PageDown, KeyCode::PageDown, KeyModifiers::NONE),
    (Key::Delete, KeyCode::Delete, KeyModifiers::NONE),
    (Key::Backspace, KeyCode::Backspace, KeyModifiers::NONE),
    (Key::Enter, KeyCode::Enter, KeyModifiers::NONE),
    (Key::Escape, KeyCode::Esc, KeyModifiers::NONE),
    (Key::Tab, KeyCode::Tab, KeyModifiers::NONE),
    (Key::Space, KeyCode::Char(' '), KeyModifiers::NONE),
    (Key::HalfPageDown, KeyCode::Char('d'), KeyModifiers::CONTROL),
    (Key::HalfPageUp, KeyCode::Char('u'), KeyModifiers::CONTROL),
    (
        Key::PreviousLocation,
        KeyCode::Char('o'),
        KeyModifiers::CONTROL,
    ),
    (Key::NextLocation, KeyCode::Char('i'), KeyModifiers::CONTROL),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_s_does_not_search_move_or_edit_in_either_keymap() {
        for keymap in [EditorKeymap::Vim, EditorKeymap::Regular] {
            let mut editor = CommentEditor::new("abc abc", keymap);
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
    fn switching_keymaps_preserves_the_draft_and_changes_escape_and_typing() {
        let mut editor = CommentEditor::new("abc", EditorKeymap::Vim);
        editor.input(Key::Right);
        editor.input(Key::EditorMode);
        assert_eq!(editor.keymap(), EditorKeymap::Regular);
        editor.input(Key::Escape);
        assert_eq!(editor.mode(), "Insert");
        editor.input(Key::Char('x'));
        assert_eq!(editor.text(), "axbc");
        editor.input(Key::Backspace);
        assert_eq!(editor.text(), "abc");

        editor.input(Key::EditorMode);
        assert_eq!(editor.keymap(), EditorKeymap::Vim);
        assert_eq!(editor.mode(), "Normal");
        editor.input(Key::First);
        editor.input(Key::Char('x'));
        assert_eq!(editor.text(), "bc");
        editor.input(Key::Char('u'));
        assert_eq!(editor.text(), "abc");
    }

    #[test]
    fn switching_keymaps_cancels_pending_vim_commands_and_search() {
        let mut editor = CommentEditor::new("abc", EditorKeymap::Vim);
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
    fn footer_highlights_the_active_keymap_and_shows_the_vim_state() {
        let mut editor = CommentEditor::new("draft", EditorKeymap::Vim);
        let palette = ui_theme::Theme::default().palette;
        let area = Rect::new(0, 0, 40, 3);
        let mut buffer = Buffer::empty(area);
        editor.render(area, &mut buffer, palette);
        let footer = |buffer: &Buffer| {
            (0..area.width)
                .map(|x| buffer[(x, 2)].symbol())
                .collect::<String>()
        };
        assert!(footer(&buffer).starts_with("Vim · INSERT"));
        assert!(footer(&buffer).ends_with("F2 · Regular editing"));
        assert_eq!(buffer[(0, 2)].fg, palette.warning);
        assert_eq!(buffer[(39, 2)].fg, palette.dim);

        editor.input(Key::Escape);
        editor.render(area, &mut buffer, palette);
        assert!(footer(&buffer).starts_with("Vim · NORMAL"));
        editor.input(Key::EditorMode);
        buffer.reset();
        editor.render(area, &mut buffer, palette);
        assert!(footer(&buffer).starts_with("Regular editing"));
        assert!(footer(&buffer).ends_with("F2 · Vim editing"));
        assert_eq!(buffer[(0, 2)].fg, palette.warning);
        assert_eq!(buffer[(39, 2)].fg, palette.dim);
        assert_eq!(editor.text(), "draft");
    }

    #[test]
    fn a_single_editor_row_keeps_the_draft_and_cursor_visible() {
        let palette = ui_theme::Theme::default().palette;
        let area = Rect::new(0, 0, 16, 1);
        for keymap in [EditorKeymap::Vim, EditorKeymap::Regular] {
            let mut editor = CommentEditor::new("draft", keymap);
            editor.input(Key::Last);
            editor.input(Key::Char('x'));
            let mut buffer = Buffer::empty(area);
            editor.render(area, &mut buffer, palette);
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
        let mut editor = CommentEditor::new("one two three", EditorKeymap::Vim);
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
        let mut editor = CommentEditor::new("one two three", EditorKeymap::Vim);
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
        let mut editor = CommentEditor::new("", EditorKeymap::Vim);
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
        let mut editor = CommentEditor::new("ac", EditorKeymap::Vim);
        editor.input(Key::Right);
        editor.paste("b\nsecond");
        assert_eq!(editor.text(), "ab\nsecondc");
    }

    #[test]
    fn paste_at_start_and_end_handles_leading_newlines() {
        let mut editor = CommentEditor::new("middle", EditorKeymap::Vim);
        editor.paste("\nfirst\n");
        assert_eq!(editor.text(), "\nfirst\nmiddle");
        editor.input(Key::Last);
        editor.paste("終");
        assert_eq!(editor.text(), "\nfirst\nmiddle終");
    }
}
