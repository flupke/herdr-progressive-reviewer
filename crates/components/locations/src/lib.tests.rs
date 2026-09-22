use std::path::PathBuf;

use component_core::{Component, ComponentEventBus, ComponentSubscriptions, EventEnvelope};
use review_guide::ReviewCheckpoint;
use review_lsp::{Event as LspEvent, Operation, SourceLocation};
use toasts::ToastId;
use ui_actions::Action;
use ui_events::{
    FilesViewportChanged, LocationListVisibilityChanged, PointerInput, PointerInputKind,
    PointerPosition, RepositoryMetadataChanged, SourceLocationAccepted,
    SourceLocationPreviewRequested,
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

fn location_list(
    operation: Operation,
) -> (ComponentEventBus<Action>, component_core::ComponentTarget) {
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
    event_bus
        .publish(LspEvent::Locations {
            toast_id: ToastId::generate(),
            operation,
            snapshot_id: "snapshot".to_owned(),
            locations: (0..4).map(location).collect(),
        })
        .expect("locations must dispatch");
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
            Action::EditRevision { change_id: text } => Some(text.as_str().to_owned()),
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
}

impl Component<Action> for LocationOutput {
    fn register_subscriptions(subscriptions: &mut ComponentSubscriptions<'_, Self, Action>) {
        subscriptions.subscribe(Self::preview);
        subscriptions.subscribe(Self::accepted);
        subscriptions.subscribe(Self::visibility);
    }
}

fn output(text: String) -> Vec<Action> {
    vec![Action::EditRevision {
        change_id: text.into(),
    }]
}
