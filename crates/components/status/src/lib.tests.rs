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
    assert!(rendered.contains("Component migration"));
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
