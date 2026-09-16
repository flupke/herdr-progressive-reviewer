use super::*;

#[test]
fn vertical_motion_follows_wrapped_rows_in_both_keymaps_without_changing_text() {
    for keymap in [EditorKeymap::Regular, EditorKeymap::Vim] {
        let text = "one two three four five six seven eight nine ten";
        let mut editor = CommentEditor::new(text, keymap);
        render(&editor, 10, 4);
        editor.input(Key::Down);
        assert_eq!(editor.state.borrow().cursor, edtui::Index2::new(0, 8));
        editor.input(Key::Down);
        assert_eq!(editor.state.borrow().cursor, edtui::Index2::new(0, 19));
        editor.input(Key::PageDown);
        assert!(editor.state.borrow().cursor.col >= 39);
        editor.input(Key::PageUp);
        assert_eq!(editor.state.borrow().cursor.row, 0);
        assert!(editor.state.borrow().cursor.col < 30);
        editor.input(Key::Up);
        editor.input(Key::Up);
        assert_eq!(editor.state.borrow().cursor, edtui::Index2::new(0, 0));
        assert_eq!(editor.text(), text);
    }
}

#[test]
fn vim_visual_rows_keep_unicode_columns_and_native_pending_find_commands() {
    let text = "é界 e\u{301} one two three four j";
    let mut editor = CommentEditor::new(text, EditorKeymap::Vim);
    editor.input(Key::Escape);
    render(&editor, 8, 5);
    editor.input(Key::Char('j'));
    assert_eq!(editor.state.borrow().cursor.row, 0);
    assert!(editor.state.borrow().cursor.col > 0);
    editor.input(Key::Char('k'));
    assert_eq!(editor.state.borrow().cursor.col, 0);
    editor.input(Key::Char('f'));
    editor.input(Key::Char('j'));
    assert_eq!(editor.state.borrow().cursor.col, text.chars().count() - 1);
    assert_eq!(editor.text(), text);
}

#[test]
fn vim_half_pages_move_within_a_wrapped_paragraph() {
    let text = "one two three four five six seven eight nine ten";
    let mut editor = CommentEditor::new(text, EditorKeymap::Vim);
    editor.input(Key::Escape);
    render(&editor, 10, 4);
    editor.input(Key::HalfPageDown);
    assert_eq!(editor.state.borrow().cursor, edtui::Index2::new(0, 8));
    editor.input(Key::HalfPageUp);
    assert_eq!(editor.state.borrow().cursor, edtui::Index2::new(0, 0));
    assert_eq!(editor.text(), text);
}

fn render(editor: &CommentEditor, width: u16, height: u16) -> Buffer {
    let area = Rect::new(0, 0, width, height);
    let mut buffer = Buffer::empty(area);
    editor.render(area, &mut buffer, ui_theme::Theme::default().palette);
    buffer
}

fn row(buffer: &Buffer, y: u16) -> String {
    (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol())
        .collect::<String>()
        .trim_end()
        .to_owned()
}

#[test]
fn typing_and_resizing_wrap_words_without_changing_the_draft() {
    for keymap in [EditorKeymap::Vim, EditorKeymap::Regular] {
        let mut editor = CommentEditor::new("", keymap);
        for c in "hello world again".chars() {
            editor.input(Key::Char(c));
            render(&editor, 8, 5);
        }
        let buffer = render(&editor, 8, 5);
        assert_eq!(row(&buffer, 0), "hello");
        assert_eq!(row(&buffer, 1), "world");
        assert_eq!(row(&buffer, 2), "again");
        assert_eq!(buffer[(5, 2)].bg, ui_theme::Theme::default().palette.focus);
        let wider = render(&editor, 20, 5);
        assert_eq!(row(&wider, 0), "hello world again");
        assert_eq!(editor.text(), "hello world again");
    }
}

#[test]
fn words_that_fill_the_row_do_not_leave_the_next_word_indented_or_split() {
    let editor = CommentEditor::new("hello   world", EditorKeymap::Vim);
    let buffer = render(&editor, 5, 5);
    assert_eq!(row(&buffer, 0), "hello");
    assert_eq!(row(&buffer, 1), "world");
    assert_eq!(editor.text(), "hello   world");
}

#[test]
fn an_end_of_line_cursor_does_not_push_a_fitting_word_to_the_next_row() {
    let mut editor = CommentEditor::new("test words", EditorKeymap::Vim);
    editor.input(Key::Last);
    let buffer = render(&editor, 10, 4);
    assert_eq!(row(&buffer, 0), "test words");
    assert_eq!(buffer[(0, 1)].bg, ui_theme::Theme::default().palette.focus);
    assert_eq!(editor.text(), "test words");
}

#[test]
fn navigating_and_editing_soft_break_spaces_keeps_the_cursor_on_a_space() {
    for steps in 5..8 {
        let mut editor = CommentEditor::new("hello   world", EditorKeymap::Vim);
        editor.input(Key::Escape);
        render(&editor, 5, 5);
        for _ in 0..steps {
            editor.input(Key::Right);
        }
        let buffer = render(&editor, 5, 5);
        let cursor = buffer
            .content()
            .iter()
            .find(|cell| cell.bg == ui_theme::Theme::default().palette.focus)
            .unwrap();
        assert_eq!(cursor.symbol(), " ");
        assert_eq!(row(&buffer, 0), "hello");
        editor.input(Key::Char('x'));
        assert_eq!(editor.text(), "hello  world");
    }
}

#[test]
fn page_motions_scroll_in_both_directions_without_getting_stuck() {
    let text = (0..10)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut editor = CommentEditor::new(&text, EditorKeymap::Vim);
    editor.input(Key::Escape);
    assert_eq!(row(&render(&editor, 12, 4), 0), "line 0");
    for expected in [3, 6, 7] {
        editor.input(Key::PageDown);
        assert_eq!(row(&render(&editor, 12, 4), 0), format!("line {expected}"));
    }
    for expected in [4, 1, 0] {
        editor.input(Key::PageUp);
        assert_eq!(row(&render(&editor, 12, 4), 0), format!("line {expected}"));
    }
    assert_eq!(editor.text(), text);
}

#[test]
fn long_words_wrap_and_the_insert_cursor_remains_visible() {
    let mut editor = CommentEditor::new("", EditorKeymap::Vim);
    for c in "abcdefghijklmnop".chars() {
        editor.input(Key::Char(c));
        let buffer = render(&editor, 5, 3);
        assert!(
            buffer
                .content()
                .iter()
                .any(|cell| cell.bg == ui_theme::Theme::default().palette.focus)
        );
    }
    let buffer = render(&editor, 5, 3);
    assert_eq!(row(&buffer, 0), "klmno");
    assert_eq!(row(&buffer, 1), "p");
    assert_eq!(editor.text(), "abcdefghijklmnop");
}

#[test]
fn unicode_and_explicit_newlines_survive_wrapping() {
    let mut editor = CommentEditor::new("", EditorKeymap::Vim);
    editor.paste("cafe\u{301} 世界\n\nnext\tword");
    editor.input(Key::Escape);
    editor.input(Key::Char('g'));
    editor.input(Key::Char('g'));
    let buffer = render(&editor, 8, 7);
    assert_eq!(row(&buffer, 0), "cafe\u{301}");
    assert!(row(&buffer, 1).starts_with('世'));
    assert_eq!(row(&buffer, 2), "");
    assert_eq!(row(&buffer, 3), "next");
    assert_eq!(row(&buffer, 4), "word");
    assert_eq!(editor.text(), "cafe\u{301} 世界\n\nnext\tword");
}

#[test]
fn vim_word_edits_undo_selection_and_search_use_original_coordinates() {
    let mut editor = CommentEditor::new("alpha beta gamma", EditorKeymap::Vim);
    render(&editor, 8, 6);
    editor.input(Key::Escape);
    for c in "wviw".chars() {
        editor.input(Key::Char(c));
    }
    let selected = render(&editor, 8, 6);
    assert_eq!(row(&selected, 1), "beta");
    assert_eq!(
        selected[(1, 1)].bg,
        ui_theme::Theme::default().palette.selection
    );
    editor.input(Key::Char('c'));
    for c in "new".chars() {
        editor.input(Key::Char(c));
    }
    editor.input(Key::Escape);
    assert_eq!(editor.text(), "alpha new gamma");
    render(&editor, 8, 6);
    editor.input(Key::Char('u'));
    assert_eq!(editor.text(), "alpha beta gamma");
    for c in "/gamma".chars() {
        editor.input(Key::Char(c));
    }
    let searched = render(&editor, 8, 6);
    assert_eq!(row(&searched, 2), "gamma");
    assert_eq!(
        searched[(1, 2)].bg,
        ui_theme::Theme::default().palette.selection
    );
    assert_eq!(editor.text(), "alpha beta gamma");
}
