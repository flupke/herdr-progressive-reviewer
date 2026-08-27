use ratatui::style::Color;

use super::FileStatistics;

#[test]
fn statistics_width_includes_values_and_the_separator() {
    for (added, removed, expected) in [
        (None, None, 0),
        (Some("+12"), None, 3),
        (None, Some("-3"), 2),
        (Some("+12"), Some("-3"), 6),
    ] {
        let statistics = FileStatistics {
            added: added.map(str::to_owned),
            removed: removed.map(str::to_owned),
        };

        assert_eq!(statistics.width(), expected);
    }
}

#[test]
fn statistics_append_both_colored_values_with_one_separator() {
    let statistics = FileStatistics {
        added: Some("+12".to_owned()),
        removed: Some("-3".to_owned()),
    };
    let mut spans = Vec::new();

    statistics.append(&mut spans, Color::Green, Color::Red);

    assert_eq!(
        spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<Vec<_>>(),
        ["+12", " ", "-3"]
    );
    assert_eq!(spans[0].style.fg, Some(Color::Green));
    assert_eq!(spans[2].style.fg, Some(Color::Red));
}
