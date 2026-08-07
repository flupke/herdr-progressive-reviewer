use ratatui::text::Line;

use super::wrap_line;

#[test]
fn continuation_indent_leaves_room_for_a_wide_grapheme() {
    let rows = wrap_line(&Line::raw("abcd🙂"), 5, 4);

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].0.width(), 5);
    assert_eq!(rows[1].0.to_string(), "   🙂");
}

#[test]
fn lines_wrap_after_spaces_instead_of_inside_words() {
    let rows = wrap_line(&Line::raw("prefix alpha beta gamma"), 13, 4);

    assert_eq!(
        rows.iter()
            .map(|(line, _)| line.to_string())
            .collect::<Vec<_>>(),
        ["prefix alpha ", "    beta ", "    gamma"]
    );
}

#[test]
fn lines_wrap_after_tabs_instead_of_inside_words() {
    let rows = wrap_line(&Line::raw("alpha\tbeta gamma"), 10, 2);

    assert_eq!(
        rows.iter()
            .map(|(line, _)| line.to_string())
            .collect::<Vec<_>>(),
        ["alpha\t", "  beta ", "  gamma"]
    );
}

#[test]
fn words_wider_than_the_viewport_use_hard_breaks() {
    let rows = wrap_line(&Line::raw("abcdefgh"), 5, 2);

    assert_eq!(
        rows.iter()
            .map(|(line, _)| line.to_string())
            .collect::<Vec<_>>(),
        ["abcde", "  fgh"]
    );
}

#[test]
fn continuation_rows_repeat_the_change_marker() {
    let rows = wrap_line(&Line::raw("▌ 1 alpha beta gamma"), 10, 4);

    assert!(
        rows.iter()
            .skip(1)
            .all(|(line, _)| line.to_string().starts_with("▌ "))
    );
}
