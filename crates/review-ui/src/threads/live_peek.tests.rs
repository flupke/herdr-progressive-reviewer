use super::*;
use std::fmt::Write;
use ui_events::LocationListVisibilityChanged;

#[test]
fn filesystem_refresh_updates_peek_without_moving_it_and_rejects_stale_loads() {
    assert_live_refresh(false);
}

#[test]
fn preview_opened_during_refresh_preserves_the_underlying_source_position() {
    assert_live_refresh(true);
}

fn assert_live_refresh(with_preview: bool) {
    let mut ui = ThreadUi::new(110);
    ui.key(Key::Char('t'));
    ui.key(Key::Enter);
    let initial = ui.key(Key::Char('p'));
    let Action::LoadSource {
        snapshot_id,
        location,
        mode,
    } = &initial[0]
    else {
        panic!("initial peek");
    };
    let original = (0..100).fold(String::new(), |mut text, index| {
        writeln!(text, "line {index}").unwrap();
        text
    });
    ui.app.publish(SourceContentLoaded {
        snapshot_id: snapshot_id.clone(),
        location: location.clone(),
        content: original.as_bytes().to_vec(),
        mode: *mode,
    });
    for _ in 0..4 {
        ui.key(Key::PageDown);
    }
    let before = ui.text();
    let actions = ui.app.publish(ui_events::RepositoryRefreshStarted);
    let Action::LoadSource {
        snapshot_id: fresh,
        location: refreshed,
        mode,
    } = &actions[0]
    else {
        panic!("live peek");
    };
    assert_ne!(snapshot_id, fresh);
    assert_eq!(location.path, refreshed.path);
    if with_preview {
        let target = review_lsp::SourceLocation {
            path: "/repo/preview.rs".into(),
            ..location.clone()
        };
        ui.preview_source(fresh, target, "temporary LSP preview\n");
    }
    ui.app.publish(SourceContentLoaded {
        snapshot_id: fresh.clone(),
        location: refreshed.clone(),
        content: original.replace("line", "live").into_bytes(),
        mode: *mode,
    });
    ui.app
        .publish(LocationListVisibilityChanged { visible: false });
    assert_eq!(ui.text(), before.replace("line ", "live "));
    assert!(ui.text().contains("live "), "{}", ui.text());
    ui.app.publish(SourceContentLoaded {
        snapshot_id: snapshot_id.clone(),
        location: location.clone(),
        content: b"outdated result".to_vec(),
        mode: *mode,
    });
    assert!(!ui.text().contains("outdated result"));
    let actions = ui.app.publish(ui_events::RepositoryRefreshStarted);
    let Action::LoadSource {
        snapshot_id: deleted,
        ..
    } = &actions[0]
    else {
        panic!("delete");
    };
    ui.app.publish(ui_events::SourceContentLoadFailed {
        snapshot_id: deleted.clone(),
        message: "file was deleted".into(),
    });
    assert!(ui.text().contains("Current file unavailable"));
    assert!(
        !ui.text().contains("live "),
        "deleted file must not retain stale code"
    );
    let actions = ui.app.publish(ui_events::RepositoryRefreshStarted);
    let Action::LoadSource {
        snapshot_id: recreated,
        ..
    } = &actions[0]
    else {
        panic!("recreate");
    };
    ui.app.publish(SourceContentLoaded {
        snapshot_id: recreated.clone(),
        location: location.clone(),
        content: original.as_bytes().to_vec(),
        mode: *mode,
    });
    assert!(ui.text().contains("line "));
}

impl ThreadUi {
    fn preview_source(
        &mut self,
        snapshot_id: &str,
        location: review_lsp::SourceLocation,
        content: &str,
    ) {
        self.app.publish(ui_events::SourceLocationPreviewRequested {
            location: location.clone(),
        });
        self.app.publish(SourceContentLoaded {
            snapshot_id: snapshot_id.into(),
            location,
            content: content.as_bytes().to_vec(),
            mode: ui_events::SourceLoadMode::Preview,
        });
    }
}

#[test]
fn live_watch_follows_definition_and_history_navigation_until_peek_closes() {
    let mut ui = ThreadUi::new(110);
    ui.key(Key::Char('t'));
    ui.key(Key::Enter);
    let initial = ui.key(Key::Char('p'));
    let Action::LoadSource {
        snapshot_id,
        location: original,
        mode,
    } = &initial[0]
    else {
        panic!("initial peek");
    };
    assert!(initial.contains(&Action::WatchSource(Some(original.path.clone()))));
    ui.app.publish(SourceContentLoaded {
        snapshot_id: snapshot_id.clone(),
        location: original.clone(),
        content: b"fn original() {}\n".to_vec(),
        mode: *mode,
    });
    let definition = review_lsp::SourceLocation {
        path: "/repo/ignored/definition.rs".into(),
        ..original.clone()
    };
    let actions = ui.app.publish(ui_events::SourceLocationAccepted {
        location: definition.clone(),
    });
    let Action::LoadSource {
        snapshot_id, mode, ..
    } = &actions[0]
    else {
        panic!("definition navigation");
    };
    let loaded = ui.app.publish(SourceContentLoaded {
        snapshot_id: snapshot_id.clone(),
        location: definition.clone(),
        content: b"fn definition() {}\n".to_vec(),
        mode: *mode,
    });
    assert!(loaded.contains(&Action::WatchSource(Some(definition.path.clone()))));

    let actions = ui.app.publish(ui_events::RepositoryRefreshStarted);
    let Action::LoadSource {
        snapshot_id,
        location,
        mode,
    } = &actions[0]
    else {
        panic!("definition refresh");
    };
    assert_eq!(location.path, definition.path);
    ui.app.publish(SourceContentLoaded {
        snapshot_id: snapshot_id.clone(),
        location: location.clone(),
        content: b"fn edited_definition() {}\n".to_vec(),
        mode: *mode,
    });
    assert!(ui.text().contains("fn edited_definition() {}"));
    let failed = ui.app.publish(ui_events::SourceContentLoadFailed {
        snapshot_id: snapshot_id.clone(),
        message: "another definition target is absent".into(),
    });
    assert!(
        !failed
            .iter()
            .any(|action| matches!(action, Action::WatchSource(_)))
    );
    assert!(ui.text().contains("fn edited_definition() {}"));

    let actions = ui.key(Key::PreviousLocation);
    let Action::LoadSource {
        snapshot_id,
        location,
        mode,
    } = &actions[0]
    else {
        panic!("history navigation: {actions:?}");
    };
    assert_eq!(location.path, original.path);
    let loaded = ui.app.publish(SourceContentLoaded {
        snapshot_id: snapshot_id.clone(),
        location: location.clone(),
        content: b"fn original() {}\n".to_vec(),
        mode: *mode,
    });
    assert!(loaded.contains(&Action::WatchSource(Some(original.path.clone()))));
    assert!(ui.key(Key::Escape).contains(&Action::WatchSource(None)));
}

#[test]
fn threads_footer_has_only_the_filter() {
    let mut ui = ThreadUi::new(110);
    ui.key(Key::Char('t'));
    let text = ui.text();
    assert!(text.contains("[Unresolved] / All"));
    assert!(!text.contains("Agent:"));
    assert!(!text.contains("Use focused agent"));
}
