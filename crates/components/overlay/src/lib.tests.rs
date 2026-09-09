use component_core::{ComponentEventBus, EventEnvelope};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use review_lsp::Event as LspEvent;
use toasts::ToastId;
use ui_actions::Action;
use ui_events::{RepositoryMetadataChanged, ViewportChanged};
use ui_shortcuts::Key;

use super::OverlayComponent;

#[test]
fn source_menu_borders_do_not_trigger_an_unlisted_lsp_operation() {
    let menu = super::SourceContextMenu {
        column: 2,
        row: 3,
        selected: 0,
        query: Some(ui_events::LspQueryContext {
            path: "src/lib.rs".into(),
            line: 0,
            byte_column: 0,
            expected_line: "struct Thing;".to_owned(),
            snapshot_id: "snapshot".to_owned(),
        }),
    };
    let area = menu.area(ratatui::layout::Rect::new(0, 0, 80, 24));
    assert_eq!(menu.clone().query_at_row(area.y, area), None);
    assert_eq!(menu.clone().query_at_row(area.bottom() - 1, area), None);
    assert_eq!(
        menu.query_at_row(area.bottom() - 2, area)
            .map(|(operation, _)| operation),
        Some(review_lsp::Operation::References)
    );
}

#[test]
fn help_opens_and_closes_through_keyboard_input() {
    let mut bus = ComponentEventBus::<Action>::new();
    let target = bus.mount(|_| OverlayComponent::new(ui_theme::Theme::default()));

    bus.dispatch_global_input(&EventEnvelope::new(Key::Char('?')))
        .unwrap();

    let mut terminal = Terminal::new(TestBackend::new(60, 14)).unwrap();
    terminal
        .draw(|frame| {
            bus.get::<OverlayComponent>(target)
                .unwrap()
                .render(frame.area(), frame.buffer_mut());
        })
        .unwrap();
    let open_screen = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect::<String>();
    assert!(open_screen.contains("Keyboard shortcuts"));

    bus.dispatch_input(&EventEnvelope::new(Key::Char('?')), target)
        .unwrap();
    terminal
        .draw(|frame| {
            bus.get::<OverlayComponent>(target)
                .unwrap()
                .render(frame.area(), frame.buffer_mut());
        })
        .unwrap();
    let closed_screen = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect::<String>();
    assert!(!closed_screen.contains("Keyboard shortcuts"));
}

#[test]
fn commit_message_renders_from_repository_event() {
    let mut bus = ComponentEventBus::<Action>::new();
    let target = bus.mount(|_| OverlayComponent::new(ui_theme::Theme::default()));
    bus.publish(ViewportChanged {
        width: 60,
        height: 14,
    })
    .unwrap();
    bus.publish(RepositoryMetadataChanged {
        display_id: "abcd1234".to_owned(),
        review_checkpoint: review_guide::ReviewCheckpoint::new("change", "snapshot"),
        description: "Explain the component migration".to_owned(),
    })
    .unwrap();
    bus.dispatch_global_input(&EventEnvelope::new(Key::CommitMessage))
        .unwrap();

    let mut terminal = Terminal::new(TestBackend::new(60, 14)).unwrap();
    terminal
        .draw(|frame| {
            bus.get::<OverlayComponent>(target)
                .unwrap()
                .render(frame.area(), frame.buffer_mut());
        })
        .unwrap();
    let screen = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect::<String>();
    assert!(screen.contains("Explain the component migration"));
}

#[test]
fn stale_hover_results_do_not_replace_the_current_overlay() {
    let mut bus = ComponentEventBus::<Action>::new();
    let target = bus.mount(|_| OverlayComponent::new(ui_theme::Theme::default()));
    bus.publish(RepositoryMetadataChanged {
        display_id: "abcd1234".to_owned(),
        review_checkpoint: review_guide::ReviewCheckpoint::new("change", "current"),
        description: String::new(),
    })
    .unwrap();

    bus.publish(LspEvent::Hover {
        toast_id: ToastId::generate(),
        snapshot_id: "stale".to_owned(),
        markdown: Some("stale documentation".to_owned()),
    })
    .unwrap();
    assert!(!rendered_screen(&bus, target).contains("stale documentation"));

    bus.publish(LspEvent::Hover {
        toast_id: ToastId::generate(),
        snapshot_id: "current".to_owned(),
        markdown: Some("current documentation".to_owned()),
    })
    .unwrap();
    assert!(rendered_screen(&bus, target).contains("current documentation"));
}

#[test]
fn stale_lsp_failures_do_not_create_toasts() {
    let mut bus = ComponentEventBus::<Action>::new();
    let target = bus.mount(|_| OverlayComponent::new(ui_theme::Theme::default()));
    bus.publish(RepositoryMetadataChanged {
        display_id: "abcd1234".to_owned(),
        review_checkpoint: review_guide::ReviewCheckpoint::new("change", "current"),
        description: String::new(),
    })
    .unwrap();

    bus.publish(LspEvent::Failed {
        toast_id: None,
        snapshot_id: Some("stale".to_owned()),
        message: "stale failure".to_owned(),
    })
    .unwrap();
    assert!(!rendered_screen(&bus, target).contains("stale failure"));

    bus.publish(LspEvent::Failed {
        toast_id: None,
        snapshot_id: Some("current".to_owned()),
        message: "current failure".to_owned(),
    })
    .unwrap();
    assert!(rendered_screen(&bus, target).contains("current failure"));
}

#[test]
fn server_startup_toasts_finish_independently() {
    let mut bus = ComponentEventBus::<Action>::new();
    let target = bus.mount(|_| OverlayComponent::new(ui_theme::Theme::default()));
    let expert = review_lsp::ServerStartup {
        id: ToastId::generate(),
        name: "expert",
    };
    let typescript = review_lsp::ServerStartup {
        id: ToastId::generate(),
        name: "typescript-language-server",
    };
    bus.publish(LspEvent::Initializing(expert)).unwrap();
    bus.publish(LspEvent::Initializing(typescript)).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(260));
    bus.publish(LspEvent::Ready(expert)).unwrap();
    let screen = rendered_screen(&bus, target);
    assert!(screen.contains("expert ready"));
    assert!(screen.contains("Starting typescript-language-server..."));

    bus.publish(LspEvent::Failed {
        toast_id: Some(typescript.id),
        snapshot_id: None,
        message: "TypeScript startup failed".to_owned(),
    })
    .unwrap();
    let screen = rendered_screen(&bus, target);
    assert!(!screen.contains("Starting typescript-language-server..."));
    assert!(screen.contains("TypeScript startup failed"));
}

fn rendered_screen(
    bus: &ComponentEventBus<Action>,
    target: component_core::ComponentTarget,
) -> String {
    let mut terminal = Terminal::new(TestBackend::new(60, 14)).unwrap();
    terminal
        .draw(|frame| {
            bus.get::<OverlayComponent>(target)
                .unwrap()
                .render_notifications(frame.area(), frame.buffer_mut());
            bus.get::<OverlayComponent>(target)
                .unwrap()
                .render(frame.area(), frame.buffer_mut());
        })
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect()
}
