use ratatui::layout::{Position, Rect};
use ui_text_selection::{RowEnd, TextCell, TextLayer, TextPress, TextRegion, TextRow};

use super::ClippedViewport;

fn row(y: u16, text: &str) -> TextRow {
    TextRow::new(
        Position::new(0, y),
        text.chars()
            .map(|character| TextCell::new(character.to_string(), 1))
            .collect(),
        RowEnd::Line,
    )
}

#[test]
fn placed_text_keeps_only_the_rows_the_viewport_shows_where_it_shows_them() {
    let mut window = TextRegion::new(Rect::new(0, 0, 30, 10));
    for (y, text) in [
        (1, "one"),
        (4, "four"),
        (5, "five"),
        (6, "six"),
        (7, "seven"),
    ] {
        window.push(row(y, text));
    }
    // Rows 4 to 6 of the window show at rows 20 to 22, from column 10.
    let viewport = ClippedViewport::new(Rect::new(10, 20, 30, 3), 4, 10);
    let mut layer = TextLayer::default();
    layer.push(viewport.place_text(&window));

    let mut selection = layer.select(Position::new(10, 20), TextPress::Alt).unwrap();
    selection.extend(Position::new(39, 22));
    assert_eq!(selection.text().as_deref(), Some("four\nfive\nsix"));
}

#[test]
fn placed_text_covers_only_where_the_viewport_shows_the_region() {
    // A thread on rows 5 to 7 of the window, under rows 4 to 6 shown at rows 20 to 22.
    let thread = TextRegion::new(Rect::new(2, 5, 20, 3));
    let viewport = ClippedViewport::new(Rect::new(10, 20, 30, 3), 4, 10);
    assert_eq!(
        viewport.place_text(&thread).area(),
        Rect::new(12, 21, 20, 2)
    );

    let hidden = TextRegion::new(Rect::new(0, 8, 30, 2));
    assert!(viewport.place_text(&hidden).area().is_empty());
}
