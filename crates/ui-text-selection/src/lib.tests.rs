use super::*;
use ratatui::style::{Color, Style};

fn cells(text: &str) -> Vec<TextCell> {
    text.chars()
        .map(|character| TextCell::new(character.to_string(), 1))
        .collect()
}

fn drawn(area: Rect, rows: &[&str]) -> Buffer {
    let mut buffer = Buffer::empty(Rect::new(0, 0, area.right() + 2, area.bottom() + 1));
    for (offset, row) in rows.iter().enumerate() {
        let y = area.y + u16::try_from(offset).unwrap();
        buffer.set_string(area.x, y, row, Style::default());
    }
    buffer
}

fn select(region: TextRegion, from: (u16, u16), to: (u16, u16)) -> Option<String> {
    let mut layer = TextLayer::default();
    layer.push(region);
    let mut selection = layer.select(Position::new(from.0, from.1), TextPress::Alt)?;
    selection.extend(Position::new(to.0, to.1));
    selection.text()
}

fn code_region() -> TextRegion {
    let mut region = TextRegion::new(Rect::new(0, 0, 20, 4));
    let mut first = vec![TextCell::new("\t", 4)];
    first.extend(cells("let a"));
    region.push(TextRow::new(
        Position::new(6, 0),
        first,
        RowEnd::Wrap { space: false },
    ));
    region.push(TextRow::new(
        Position::new(6, 1),
        cells(" = 1;"),
        RowEnd::Line,
    ));
    region.push(TextRow::new(
        Position::new(6, 3),
        cells("b();"),
        RowEnd::Line,
    ));
    region
}

#[test]
fn a_drag_copies_content_rows_joining_wrapped_ones_and_keeping_tabs() {
    assert_eq!(
        select(code_region(), (0, 0), (19, 3)).as_deref(),
        Some("\tlet a = 1;\nb();")
    );
}

#[test]
fn a_drag_copies_from_the_anchor_cell_to_the_cursor_cell_in_reading_order() {
    // Dragged upwards, from the `1` back to the `e` of `let`.
    assert_eq!(
        select(code_region(), (9, 1), (11, 0)).as_deref(),
        Some("et a = 1")
    );
}

#[test]
fn a_cell_wider_than_one_column_is_selected_from_any_of_its_columns() {
    // Column 8 is inside the tab, which spans columns 6 to 9.
    assert_eq!(
        select(code_region(), (8, 0), (10, 0)).as_deref(),
        Some("\tl")
    );
}

#[test]
fn a_click_without_a_drag_copies_nothing() {
    assert_eq!(select(code_region(), (7, 0), (7, 0)), None);
}

#[test]
fn the_cursor_stays_inside_the_region_of_the_anchor() {
    let mut layer = TextLayer::default();
    layer.push(TextRegion::screen(
        Rect::new(0, 0, 3, 1),
        &drawn(Rect::new(0, 0, 3, 1), &["abc"]),
    ));
    let mut region = TextRegion::new(Rect::new(4, 0, 3, 1));
    region.push(TextRow::new(
        Position::new(4, 0),
        cells("xyz"),
        RowEnd::Line,
    ));
    layer.push(region);
    let mut selection = layer.select(Position::new(5, 0), TextPress::Alt).unwrap();
    selection.extend(Position::new(0, 5));
    assert_eq!(selection.text().as_deref(), Some("xy"));
}

#[test]
fn a_text_selection_starts_in_the_region_drawn_last() {
    let mut layer = TextLayer::default();
    let area = Rect::new(0, 0, 10, 1);
    layer.push(TextRegion::screen(area, &drawn(area, &["underneath"])));
    let mut popup = TextRegion::new(Rect::new(2, 0, 4, 1));
    popup.push(TextRow::new(
        Position::new(2, 0),
        cells("menu"),
        RowEnd::Line,
    ));
    layer.push(popup);
    let mut selection = layer.select(Position::new(2, 0), TextPress::Alt).unwrap();
    selection.extend(Position::new(9, 0));
    assert_eq!(selection.text().as_deref(), Some("menu"));
}

#[test]
fn a_plain_press_starts_a_text_selection_only_where_no_region_on_top_needs_alt() {
    let mut layer = TextLayer::default();
    let area = Rect::new(0, 0, 30, 6);
    layer.push(TextRegion::screen(area, &drawn(area, &["files"])));
    // The code region covers the files from column 0 to 19 and row 0 to 3.
    layer.push(code_region().needing_alt());
    assert!(
        layer
            .select(Position::new(25, 5), TextPress::Plain)
            .is_some()
    );
    assert!(
        layer
            .select(Position::new(6, 0), TextPress::Plain)
            .is_none()
    );
    assert!(layer.select(Position::new(6, 0), TextPress::Alt).is_some());
}

#[test]
fn a_placed_region_still_needs_alt() {
    let mut layer = TextLayer::default();
    layer.push(
        code_region()
            .needing_alt()
            .placed(Rect::new(0, 10, 20, 4), |position| {
                Some(Position::new(position.x, position.y + 10))
            }),
    );
    assert!(
        layer
            .select(Position::new(6, 10), TextPress::Plain)
            .is_none()
    );
}

#[test]
fn the_highlight_paints_the_selected_content_and_not_the_gutter() {
    let mut layer = TextLayer::default();
    layer.push(code_region());
    let mut selection = layer.select(Position::new(0, 1), TextPress::Alt).unwrap();
    selection.extend(Position::new(7, 1));
    let mut buffer = Buffer::empty(Rect::new(0, 0, 20, 4));
    let style = Style::default().bg(Color::Blue);
    selection.highlight(&mut buffer, style);
    let painted = (0..20)
        .filter(|x| buffer[(*x, 1)].bg == Color::Blue)
        .collect::<Vec<_>>();
    assert_eq!(painted, [6, 7]);
}

#[test]
fn a_click_without_a_drag_paints_nothing() {
    let mut layer = TextLayer::default();
    layer.push(code_region());
    let selection = layer.select(Position::new(7, 1), TextPress::Plain).unwrap();
    let mut buffer = Buffer::empty(Rect::new(0, 0, 20, 4));
    selection.highlight(&mut buffer, Style::default().bg(Color::Blue));
    assert!(buffer.content().iter().all(|cell| cell.bg != Color::Blue));
}

#[test]
fn screen_rows_drop_trailing_blanks_and_read_wide_graphemes_once() {
    let area = Rect::new(1, 1, 12, 2);
    let region = TextRegion::screen(area, &drawn(area, &["搜索 file   ", "  next"]));
    assert_eq!(
        select(region, (1, 1), (12, 2)).as_deref(),
        Some("搜索 file\n  next")
    );
}

fn prose(rows: &[&str], width: u16) -> String {
    let area = Rect::new(2, 1, width, u16::try_from(rows.len()).unwrap());
    let mut region = TextRegion::new(area);
    region.push_prose(area, &drawn(area, rows));
    select(region, (area.x, area.y), (area.right(), area.bottom() - 1)).unwrap()
}

#[test]
fn prose_joins_a_row_whose_next_word_did_not_fit_with_the_space_the_wrap_dropped() {
    assert_eq!(
        prose(&["The quick brown", "fox jumps."], 18),
        "The quick brown fox jumps."
    );
}

#[test]
fn prose_keeps_a_line_break_where_the_next_word_would_have_fit() {
    assert_eq!(
        prose(&["Short line.", "Next line."], 18),
        "Short line.\nNext line."
    );
}

#[test]
fn prose_drops_the_indent_of_a_list_item_continuation_and_starts_each_item_anew() {
    assert_eq!(
        prose(&["•  first item goes", "   on here", "•  second item"], 19,),
        "•  first item goes on here\n•  second item"
    );
}

#[test]
fn prose_joins_a_word_cut_at_the_edge_without_a_space() {
    assert_eq!(
        prose(&["see https://exampl", "e.com/a/b now"], 18),
        "see https://example.com/a/b now"
    );
}

#[test]
fn prose_leaves_out_code_frame_borders_and_rules_and_never_joins_code() {
    assert_eq!(
        prose(
            &[
                "╭─ rust ─────────╮",
                "│ let value = 10;│",
                "│ value          │",
                "╰────────────────╯",
            ],
            18,
        ),
        "let value = 10;\nvalue"
    );
}

#[test]
fn prose_leaves_out_a_rule_that_runs_in_from_outside_the_area() {
    // A reply editor framed over the padding of a thread whose prose starts at column 3.
    let mut buffer = Buffer::empty(Rect::new(0, 0, 30, 3));
    for (y, row) in ["│  Why the tab", "│╰─ Vim · INSERT ──── F2 ─╯", "│  Post"]
        .iter()
        .enumerate()
    {
        buffer.set_string(0, u16::try_from(y).unwrap(), row, Style::default());
    }
    let area = Rect::new(3, 0, 20, 3);
    let mut region = TextRegion::new(area);
    region.push_prose(area, &buffer);
    assert_eq!(
        select(region, (3, 0), (22, 2)).as_deref(),
        Some("Why the tab\nPost")
    );
}

#[test]
fn prose_keeps_blank_rows_between_paragraphs() {
    assert_eq!(prose(&["One.", "", "Two."], 18), "One.\n\nTwo.");
}
