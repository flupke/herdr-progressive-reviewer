use component_core::ComponentEventBus;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ui_actions::Action;
use ui_events::{FilesOverviewChanged, RepositoryMetadataChanged, SearchStatusChanged};
use ui_theme::Theme;

use super::StatusComponent;

#[test]
fn external_events_change_visible_header_output() {
    let mut bus = ComponentEventBus::<Action>::new();
    let target = bus.mount(StatusComponent::new);
    bus.publish(RepositoryMetadataChanged {
        display_id: "\x1b[31mab\x1b[90mcd1234\x1b[0m".to_owned(),
        review_checkpoint: review_guide::ReviewCheckpoint::new("change", "snapshot"),
        description: "Component migration\nbody".to_owned(),
    })
    .unwrap();
    bus.publish(FilesOverviewChanged {
        reviewed: 2,
        total: 5,
        lines_added: 12,
        lines_removed: 3,
    })
    .unwrap();
    let mut buffer = Buffer::empty(Rect::new(0, 0, 80, 1));
    bus.get::<StatusComponent>(target).unwrap().render_header(
        buffer.area,
        &mut buffer,
        Theme::default().palette,
    );
    let rendered = buffer
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect::<String>();
    assert!(rendered.starts_with(" abcd1234 Component migration"));
    assert_eq!(buffer[(1, 0)].fg, ratatui::style::Color::Red);
    assert_eq!(buffer[(3, 0)].fg, ratatui::style::Color::DarkGray);
    assert_eq!(buffer[(10, 0)].fg, Theme::default().palette.text);
    assert!(rendered.contains("+12 -3 - 2/5 reviewed"));
}

#[test]
fn active_search_replaces_the_output_status_with_match_position() {
    let mut bus = ComponentEventBus::<Action>::new();
    let target = bus.mount(StatusComponent::new);
    bus.publish(SearchStatusChanged {
        query: Some("needle".to_owned()),
        current_match: 2,
        total_matches: 5,
    })
    .unwrap();
    let mut buffer = Buffer::empty(Rect::new(0, 0, 80, 1));
    bus.get::<StatusComponent>(target).unwrap().render_footer(
        buffer.area,
        &mut buffer,
        Theme::default().palette,
    );
    let rendered = buffer
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect::<String>();
    assert!(rendered.starts_with("/needle"));
    assert!(rendered.ends_with("[2/5]"));
    assert!(!rendered.contains("Output:"));
}

#[test]
fn long_subject_leaves_revision_id_and_statistics_visible() {
    let mut bus = ComponentEventBus::<Action>::new();
    let target = bus.mount(StatusComponent::new);
    bus.publish(RepositoryMetadataChanged {
        display_id: "abcd1234".to_owned(),
        review_checkpoint: review_guide::ReviewCheckpoint::new("change", "snapshot"),
        description: "A long subject repeated many times ".repeat(10),
    })
    .unwrap();
    let mut buffer = Buffer::empty(Rect::new(0, 0, 40, 1));
    bus.get::<StatusComponent>(target).unwrap().render_header(
        buffer.area,
        &mut buffer,
        Theme::default().palette,
    );
    let rendered = buffer
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect::<String>();
    assert!(rendered.starts_with(" abcd1234 A long"));
    assert!(rendered.ends_with("+0 -0 - 0/0 reviewed "));
}
