use ratatui::style::{Color, Modifier};
use ratatui::text::Line;

use super::{code_span_style, token_boundaries, wrap_line};

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

#[test]
fn token_boundaries_clip_overlapping_ranges_and_select_the_cursor_character() {
    assert_eq!(
        token_boundaries(
            2..8,
            &[0..3, 4..6, 7..10, 8..12],
            Some(&(1..5)),
            Some(6),
            "012345é6789",
        ),
        vec![2, 3, 4, 5, 6, 7, 8]
    );
}

#[test]
fn token_boundaries_ignore_adjacent_ranges_and_invalid_cursor_columns() {
    assert_eq!(
        token_boundaries(
            2..8,
            &[0..1, 0..2, 8..10, 9..10],
            Some(&(8..9)),
            Some(7),
            "012345é6789",
        ),
        vec![2, 8]
    );
}

#[test]
fn token_boundaries_add_only_cursor_columns_inside_the_token() {
    assert_eq!(
        token_boundaries(2..8, &[], None, Some(6), "012345é6789"),
        vec![2, 6, 8]
    );
    for column in [0, 1, 8, 9] {
        assert_eq!(
            token_boundaries(2..8, &[], None, Some(column), "012345é6789"),
            vec![2, 8]
        );
    }
}

#[test]
fn code_span_style_distinguishes_selection_cursor_and_plain_text() {
    let selection_match = 2..6;
    let plain = code_span_style(Color::Blue, 3..5, &[], None, None);
    let selected = code_span_style(
        Color::Blue,
        3..5,
        std::slice::from_ref(&selection_match),
        None,
        None,
    );
    let source_selected = code_span_style(Color::Blue, 3..5, &[], Some(&(3..5)), None);
    let cursor = code_span_style(Color::Blue, 3..5, &[], None, Some(3));

    assert_eq!(plain.fg, Some(Color::Blue));
    assert!(!plain.add_modifier.contains(Modifier::REVERSED));
    assert!(selected.add_modifier.contains(Modifier::REVERSED));
    assert!(source_selected.add_modifier.contains(Modifier::REVERSED));
    assert!(cursor.add_modifier.contains(Modifier::REVERSED));
    assert!(cursor.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn code_span_style_does_not_select_partial_overlaps() {
    for selection_match in [2..4, 4..6] {
        let style = code_span_style(
            Color::Blue,
            3..5,
            std::slice::from_ref(&selection_match),
            None,
            None,
        );
        assert!(!style.add_modifier.contains(Modifier::REVERSED));
    }
    for selection in [2..4, 4..6] {
        let style = code_span_style(Color::Blue, 3..5, &[], Some(&selection), None);
        assert!(!style.add_modifier.contains(Modifier::REVERSED));
    }
}
