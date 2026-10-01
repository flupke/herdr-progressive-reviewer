use component_core::ComponentEventBus;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
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
        review_checkpoint: review_source::ReviewCheckpoint::new("change", "snapshot"),
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
    let palette = Theme::default().palette;
    assert!(rendered.starts_with(" abcd1234  Component migration"));
    assert_eq!(buffer[(1, 0)].fg, ratatui::style::Color::Red);
    assert_eq!(buffer[(3, 0)].fg, ratatui::style::Color::DarkGray);
    assert_eq!(buffer[(11, 0)].fg, palette.text);
    assert!(buffer[(11, 0)].modifier.contains(Modifier::BOLD));
    // Two of five files reviewed fill four of the bar's twelve cells.
    assert!(rendered.ends_with("+12 -3  ━━━━━━━━━━━━ 2/5 reviewed "));
    let bar = u16::try_from(rendered.chars().count() - 26).unwrap();
    assert_eq!(buffer[(bar + 3, 0)].fg, palette.insertion);
    assert_eq!(buffer[(bar + 4, 0)].fg, palette.border);
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
    assert!(!rendered.contains("open"));
    assert!(!rendered.contains("new replies"));
    assert!(!rendered.contains("waiting"));
}

#[test]
fn long_subject_leaves_revision_id_and_statistics_visible() {
    let mut bus = ComponentEventBus::<Action>::new();
    let target = bus.mount(StatusComponent::new);
    bus.publish(RepositoryMetadataChanged {
        display_id: "abcd1234".to_owned(),
        review_checkpoint: review_source::ReviewCheckpoint::new("change", "snapshot"),
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
    assert!(rendered.starts_with(" abcd1234  A long"));
    // A narrow header keeps the title's room instead of drawing the bar.
    assert!(rendered.ends_with("+0 -0  0/0 reviewed "));
}

fn footer(
    bus: &ComponentEventBus<Action>,
    target: component_core::ComponentTarget,
    width: u16,
) -> String {
    let mut buffer = Buffer::empty(Rect::new(0, 0, width, 1));
    bus.get::<StatusComponent>(target).unwrap().render_footer(
        buffer.area,
        &mut buffer,
        Theme::default().palette,
    );
    buffer
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect::<String>()
}

#[test]
fn the_footer_points_to_help_with_its_key_in_the_accent() {
    let mut bus = ComponentEventBus::<Action>::new();
    let target = bus.mount(StatusComponent::new);
    let palette = Theme::default().palette;
    let mut buffer = Buffer::empty(Rect::new(0, 0, 40, 1));
    bus.get::<StatusComponent>(target)
        .unwrap()
        .render_footer(buffer.area, &mut buffer, palette);

    assert_eq!(footer(&bus, target, 40).trim_end(), "? help");
    assert_eq!(buffer[(0, 0)].fg, palette.focus);
    assert_eq!(buffer[(2, 0)].fg, palette.dim);
}
