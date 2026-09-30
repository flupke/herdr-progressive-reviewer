use ratatui::layout::Rect;
use unicode_width::UnicodeWidthStr;

use super::{HelpTable, ShortcutHelpOverlay, wrap_words};

const LONG: &str = "Switch Files / Threads / Explore, including while composing";

#[test]
fn wide_terminals_show_every_description_on_one_row() {
    let area = ShortcutHelpOverlay::area(Rect::new(0, 0, 200, 200));
    let rows = HelpTable::new().rows(area.width - 2);

    assert!(rows.iter().any(|row| row.ends_with(LONG)));
    assert!(
        rows.iter()
            .all(|row| row.width() <= usize::from(area.width - 2))
    );
}

#[test]
fn narrow_terminals_wrap_descriptions_and_scroll_through_every_row() {
    let viewport = Rect::new(0, 0, 60, 8);
    let area = ShortcutHelpOverlay::area(viewport);
    let rows = HelpTable::new().rows(area.width - 2);
    let text = rows
        .iter()
        .map(|row| row.trim())
        .collect::<Vec<_>>()
        .join(" ");

    assert!(text.contains(LONG));
    assert!(rows.len() > HelpTable::new().lines.len());
    assert_eq!(
        usize::from(ShortcutHelpOverlay::maximum_scroll(viewport)),
        rows.len() - usize::from(area.height - 2)
    );
}

#[test]
fn minimum_width_stacks_keys_above_readable_descriptions() {
    let area = ShortcutHelpOverlay::area(Rect::new(0, 0, 40, 6));
    let rows = HelpTable::new().rows(area.width - 2);
    let text = rows
        .iter()
        .map(|row| row.trim())
        .collect::<Vec<_>>()
        .join(" ");

    assert!(text.contains(&format!("Ctrl-t {LONG}")), "{rows:#?}");
    assert!(rows.contains(&"Up / Down / j / k".to_owned()));
    assert!(
        rows.iter()
            .all(|row| row.width() <= usize::from(area.width - 2))
    );
}

#[test]
fn wrap_words_keeps_words_whole() {
    assert_eq!(wrap_words("a bb ccc", 4), ["a bb", "ccc"]);
    assert_eq!(wrap_words("toolongword x", 3), ["toolongword", "x"]);
    assert_eq!(wrap_words("日本 語", 4), ["日本", "語"]);
}
