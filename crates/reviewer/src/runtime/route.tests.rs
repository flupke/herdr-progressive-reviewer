use herdr_client::protocol::WorkspaceId;
use review_ui::Key;

use super::*;

fn agent_detected() -> HerdrEvent {
    HerdrEvent::AgentDetected {
        pane_id: PaneId("agent".into()),
        workspace_id: WorkspaceId("workspace".into()),
        agent: Some("codex".into()),
        released: false,
        final_status: None,
    }
}

#[derive(Debug, PartialEq)]
struct Decision {
    route: String,
    needs_frame: bool,
    resets_cursor: bool,
}

fn decide(event: &EventEnvelope) -> Decision {
    let route = Route::of(event);
    Decision {
        // The variant name, without its payload.
        route: format!("{route:?}")
            .split(['(', ' '])
            .next()
            .unwrap()
            .to_owned(),
        needs_frame: route.needs_frame(),
        resets_cursor: route.resets_cursor(),
    }
}

#[test]
fn each_runtime_event_has_one_route_frame_and_cursor_decision() {
    let decision = |route: &str, needs_frame, resets_cursor| Decision {
        route: route.to_owned(),
        needs_frame,
        resets_cursor,
    };
    let cases = [
        (
            EventEnvelope::new(WorkerStopped),
            decision("Fail", true, false),
        ),
        (
            EventEnvelope::new(TerminalFailed("closed".into())),
            decision("Fail", true, false),
        ),
        (
            EventEnvelope::new(StopRequested),
            decision("Stop", true, false),
        ),
        (
            EventEnvelope::new(HerdrEvent::PaneFocused(PaneId("agent".into()))),
            decision("AgentFocused", true, true),
        ),
        (
            EventEnvelope::new(agent_detected()),
            decision("Herdr", false, false),
        ),
        (
            EventEnvelope::new(RepositoryRefreshDue),
            decision("RefreshDue", true, false),
        ),
        (
            EventEnvelope::new(ApplicationTick(Instant::now())),
            decision("Tick", true, false),
        ),
        (
            EventEnvelope::new(UserInput::Key(Key::Down)),
            decision("Input", true, true),
        ),
        (
            EventEnvelope::new(review_lsp::Event::Failed {
                toast_id: None,
                snapshot_id: None,
                message: "no server".into(),
            }),
            decision("Lsp", true, false),
        ),
        (
            EventEnvelope::new(TerminalFocused),
            decision("TerminalFocused", true, true),
        ),
        (
            EventEnvelope::new(ui_events::RepositoryRefreshFinished),
            decision("Application", true, false),
        ),
    ];

    for (event, expected) in cases {
        assert_eq!(decide(&event), expected, "{}", event.type_name());
    }
}

#[test]
fn failures_explain_why_the_runtime_stops() {
    let messages = [
        EventEnvelope::new(WorkerStopped),
        EventEnvelope::new(TerminalFailed("closed".into())),
    ]
    .iter()
    .map(|event| match Route::of(event) {
        Route::Fail(message) => message,
        _ => panic!("{} must fail", event.type_name()),
    })
    .collect::<Vec<_>>();

    assert_eq!(
        messages,
        [
            "review worker stopped unexpectedly",
            "could not read terminal input: closed"
        ]
    );
}
