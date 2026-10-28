use std::path::PathBuf;

use component_core::{Component, ComponentEventBus, ComponentSubscriptions, EventEnvelope};
use review_lsp::{Event as LspEvent, Operation, SourceLocation};
use review_source::ReviewCheckpoint;
use toasts::ToastId;
use ui_actions::{Action, RepositoryAction};
use ui_events::{
    FilesViewportChanged, LocationListVisibilityChanged, PointerInput, PointerInputKind,
    PointerPosition, RepositoryMetadataChanged, SourceLocationAccepted,
    SourceLocationPreviewRequested, ToastRequested,
};
use ui_shortcuts::Key;

use super::LocationsComponent;

#[test]
fn pointer_selection_previews_the_location_at_the_clicked_row() {
    let (mut event_bus, target) = location_list(Operation::References);

    let results = event_bus
        .dispatch_hovered_input(
            &EventEnvelope::new(pointer_input(PointerInputKind::Click, 3)),
            target,
        )
        .expect("pointer selection must dispatch")
        .into_results();

    assert_eq!(output_texts(results), ["preview:2"]);
}

#[test]
fn pointer_scroll_moves_the_selection_and_double_click_accepts_and_closes() {
    let (mut event_bus, target) = location_list(Operation::References);

    let scrolled = event_bus
        .dispatch_hovered_input(
            &EventEnvelope::new(PointerInput {
                kind: PointerInputKind::Scroll(1),
                position: None,
            }),
            target,
        )
        .expect("pointer scroll must dispatch")
        .into_results();
    assert_eq!(output_texts(scrolled), ["preview:1"]);

    let accepted = event_bus
        .dispatch_hovered_input(
            &EventEnvelope::new(pointer_input(PointerInputKind::DoubleClick, 3)),
            target,
        )
        .expect("double-click selection must dispatch")
        .into_results();
    assert_eq!(
        output_texts(accepted),
        ["preview:2", "accepted:2", "visible:false"]
    );
}

fn pointer_input(kind: PointerInputKind, row: u16) -> PointerInput {
    PointerInput {
        kind,
        position: Some(PointerPosition {
            terminal_column: 1,
            terminal_row: row,
            component_column: 1,
            component_row: row,
        }),
    }
}

#[test]
fn keyboard_navigation_previews_each_location_and_accepts_the_selection() {
    for operation in [Operation::References, Operation::TypeDefinition] {
        let (mut event_bus, target) = location_list(operation);

        let moved = dispatch_key(&mut event_bus, target, Key::Last);
        assert_eq!(output_texts(moved), ["preview:3"]);

        let moved = dispatch_key(&mut event_bus, target, Key::Up);
        assert_eq!(output_texts(moved), ["preview:2"]);

        let accepted = dispatch_key(&mut event_bus, target, Key::Enter);
        assert_eq!(output_texts(accepted), ["accepted:2", "visible:false"]);
    }
}

#[test]
fn a_list_with_unindexed_files_says_it_may_be_incomplete() {
    let (mut event_bus, target) = mounted_locations();
    let results = event_bus
        .publish(LspEvent::Locations {
            toast_id: ToastId::generate(),
            operation: Operation::References,
            snapshot_id: "snapshot".to_owned(),
            locations: (0..4).map(location).collect(),
            unindexed_files: 2,
        })
        .expect("locations must dispatch");
    assert!(
        output_texts(results).contains(
            &"toast:The list may be incomplete. The language server has not indexed 2 files \
              that changed on disk; gR restarts it to index them."
                .to_owned()
        )
    );
    assert!(rendered_title(&event_bus, target).contains("References (may be incomplete)"));

    let results = event_bus
        .publish(LspEvent::Locations {
            toast_id: ToastId::generate(),
            operation: Operation::References,
            snapshot_id: "snapshot".to_owned(),
            locations: Vec::new(),
            unindexed_files: 1,
        })
        .expect("locations must dispatch");
    assert_eq!(
        output_texts(results),
        [
            "toast:No locations found. The language server has not indexed 1 file that changed \
          on disk; gR restarts it to index it."
        ]
    );
}

#[test]
fn a_complete_list_keeps_its_plain_title() {
    let (event_bus, target) = location_list(Operation::References);
    let title = rendered_title(&event_bus, target);
    assert!(title.contains("References"), "{title}");
    assert!(!title.contains("incomplete"), "{title}");
}

/// The top row of the list, which holds its title.
fn rendered_title(
    event_bus: &ComponentEventBus<Action>,
    target: component_core::ComponentTarget,
) -> String {
    let area = ratatui::layout::Rect::new(0, 0, 60, 6);
    let mut buffer = ratatui::buffer::Buffer::empty(area);
    event_bus
        .get::<LocationsComponent>(target)
        .expect("the list is mounted")
        .render(area, &mut buffer, true);
    (0..area.width)
        .map(|column| buffer[(column, 0)].symbol())
        .collect()
}

fn location_list(
    operation: Operation,
) -> (ComponentEventBus<Action>, component_core::ComponentTarget) {
    let (mut event_bus, target) = mounted_locations();
    event_bus
        .publish(LspEvent::Locations {
            toast_id: ToastId::generate(),
            operation,
            snapshot_id: "snapshot".to_owned(),
            locations: (0..4).map(location).collect(),
            unindexed_files: 0,
        })
        .expect("locations must dispatch");
    (event_bus, target)
}

/// A list component on the review's snapshot, with nothing to show yet.
fn mounted_locations() -> (ComponentEventBus<Action>, component_core::ComponentTarget) {
    let mut event_bus = ComponentEventBus::new();
    let target = event_bus.mount(|events| {
        LocationsComponent::new(
            events,
            PathBuf::from("/repository"),
            ui_theme::Theme::default().palette,
        )
    });
    event_bus.mount(|_| LocationOutput::default());
    event_bus
        .publish(RepositoryMetadataChanged {
            display_id: "abcd1234".to_owned(),
            review_checkpoint: ReviewCheckpoint::new("change", "snapshot"),
            description: String::new(),
        })
        .expect("repository metadata must dispatch");
    event_bus
        .publish(FilesViewportChanged { rows: 3 })
        .expect("viewport must dispatch");
    (event_bus, target)
}

fn location(line: u32) -> SourceLocation {
    SourceLocation {
        path: PathBuf::from("/repository/src/lib.rs"),
        line,
        byte_column: 0,
        end_line: line,
        end_byte_column: 1,
    }
}

fn dispatch_key(
    event_bus: &mut ComponentEventBus<Action>,
    target: component_core::ComponentTarget,
    key: Key,
) -> Vec<component_core::DispatchResult<Action>> {
    event_bus
        .dispatch_input(&EventEnvelope::new(key), target)
        .expect("keyboard input must dispatch")
        .into_results()
}

fn output_texts(results: Vec<component_core::DispatchResult<Action>>) -> Vec<String> {
    results
        .into_iter()
        .flat_map(component_core::DispatchResult::into_actions)
        .filter_map(|action| match action {
            Action::Repository(RepositoryAction::EditRevision { change_id: text }) => {
                Some(text.as_str().to_owned())
            }
            _ => None,
        })
        .collect()
}

#[derive(Default)]
struct LocationOutput {
    observed_events: usize,
}

impl LocationOutput {
    fn preview(&mut self, event: &SourceLocationPreviewRequested) -> Vec<Action> {
        self.observed_events += 1;
        output(format!("preview:{}", event.location.line))
    }

    fn accepted(&mut self, event: &SourceLocationAccepted) -> Vec<Action> {
        self.observed_events += 1;
        output(format!("accepted:{}", event.location.line))
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn visibility(&mut self, event: &LocationListVisibilityChanged) -> Vec<Action> {
        self.observed_events += 1;
        output(format!("visible:{}", event.visible))
    }

    fn toast(&mut self, event: &ToastRequested) -> Vec<Action> {
        self.observed_events += 1;
        output(format!("toast:{}", event.text))
    }
}

impl Component<Action> for LocationOutput {
    fn register_subscriptions(subscriptions: &mut ComponentSubscriptions<'_, Self, Action>) {
        subscriptions.subscribe(Self::preview);
        subscriptions.subscribe(Self::accepted);
        subscriptions.subscribe(Self::visibility);
        subscriptions.subscribe(Self::toast);
    }
}

fn output(text: String) -> Vec<Action> {
    vec![Action::Repository(RepositoryAction::EditRevision {
        change_id: text.into(),
    })]
}
