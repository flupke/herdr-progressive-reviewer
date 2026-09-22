use super::*;
use review_explore::{ConversationBinding, DispatchId, DispatchResult, DispatchState};

fn agent() -> herdr_client::protocol::Agent {
    serde_json::from_value(serde_json::json!({
        "pane_id":"pane", "tab_id":"tab", "workspace_id":"workspace", "agent":"codex", "agent_status":"idle",
        "agent_session":{"source":"native", "agent":"codex", "kind":"id", "value":"same-conversation"}
    })).unwrap()
}

fn implementation(
    fixture: &mut Investigation,
) -> (review_explore::ImplementationRequest, DispatchId) {
    let turn = fixture.request(None, None);
    let mut conclusion = Investigation::update(&turn, 1);
    conclusion.next = None;
    conclusion.conclusion = Some(Conclusion {
        summary: "Inspect Files next".into(),
        to_be_implemented: "Generated tasks".into(),
        future_work: "Later tasks".into(),
    });
    fixture.submit(conclusion);
    fixture.mutate(|pass| {
        pass.completion = Some(review_explore::ReviewCompletion {
            request: turn.request.clone(),
            baseline: "checkpoint".into(),
            marks: vec![],
            completed: true,
            exclusions_enabled: false,
            summary: review_explore::CoverageSummary::default(),
        });
        Ok(())
    });
    let request = fixture
        .pass
        .exploration
        .implementation("Only the edited task".into())
        .unwrap();
    fixture.mutate(|pass| {
        pass.binding = ConversationBinding::from_agent(&agent());
        pass.authorize(&request).map_err(|e| e.to_string())
    });
    let id = DispatchId::Implementation {
        request: request.delivery.clone(),
        attempt: fixture.pass.implementations[&request.delivery]
            .attempt
            .clone(),
    };
    (request, id)
}

#[test]
fn archived_attempt_keeps_authoritative_success_and_cancel_cannot_rewrite_it() {
    let mut fixture = Investigation::new();
    let (request, id) = implementation(&mut fixture);
    fixture.mutate(|pass| {
        pass.begin_dispatch(&id, &agent())
            .map_err(|e| e.to_string())
    });
    let pass = fixture
        .store
        .finish_explore_dispatch(
            &"review".into(),
            &request.instance,
            &DispatchResult {
                id: id.clone(),
                began: false,
                state: DispatchState::Cancelled,
            },
        )
        .unwrap();
    assert_eq!(
        pass.implementations[&request.delivery].state,
        DispatchState::Attempting,
        "A second observer's cancellation cannot cancel the first observer's external attempt"
    );
    fixture
        .store
        .create_explore(ExplorePass::new(Exploration::new(
            fixture.pass.exploration.comparison.clone(),
        )))
        .unwrap();
    for state in [DispatchState::Delivered, DispatchState::Cancelled] {
        let pass = fixture
            .store
            .finish_explore_dispatch(
                &"review".into(),
                &request.instance,
                &DispatchResult {
                    id: id.clone(),
                    began: true,
                    state,
                },
            )
            .unwrap();
        assert_eq!(
            pass.implementations[&request.delivery].state,
            DispatchState::Delivered
        );
        assert_eq!(
            pass.implementations[&request.delivery].request.text,
            "Only the edited task"
        );
    }
}

#[test]
fn superseded_attempt_cannot_change_new_attempt_or_authorized_payload() {
    let mut fixture = Investigation::new();
    let (request, old) = implementation(&mut fixture);
    fixture.mutate(|pass| pass.authorize(&request).map_err(|e| e.to_string()));
    let pass = fixture
        .store
        .finish_explore_dispatch(
            &"review".into(),
            &request.instance,
            &DispatchResult {
                id: old,
                began: false,
                state: DispatchState::Cancelled,
            },
        )
        .unwrap();
    assert_eq!(
        pass.implementations[&request.delivery].state,
        DispatchState::Queued
    );
    let mut changed = request.clone();
    changed.text = "Expanded scope".into();
    assert!(
        fixture
            .store
            .update_explore(&"review".into(), &request.instance, |pass| pass
                .authorize(&changed)
                .map_err(|e| e.to_string()))
            .is_err()
    );
}

#[test]
fn simultaneous_posts_validate_latest_revision_instead_of_overwriting_each_other() {
    let fixture = Investigation::new();
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let workers: Vec<_> = (0..2)
        .map(|_| {
            let request = fixture
                .pass
                .exploration
                .clone()
                .request(None, None)
                .unwrap();
            let store = fixture.store.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store
                    .update_explore(&"review".into(), &request.instance, |pass| {
                        pass.post(&request).map_err(|e| e.to_string())
                    })
                    .is_ok()
            })
        })
        .collect();
    assert_eq!(
        workers
            .into_iter()
            .map(|worker| usize::from(worker.join().unwrap()))
            .sum::<usize>(),
        1
    );
    let pass = fixture
        .store
        .load_explore(&"review".into(), &fixture.pass.exploration.instance)
        .unwrap()
        .unwrap();
    assert_eq!(pass.turns.len(), 1);
    assert!(pass.exploration.pending_request().is_some());
}

#[test]
fn a_cancelled_turn_cannot_dispatch_or_replace_a_newer_post_on_retry() {
    let mut fixture = Investigation::new();
    let old = fixture.request(None, None);
    let id = DispatchId::Interview {
        request: old.request.clone(),
        attempt: fixture.pass.turns[&old.request].attempt.clone(),
    };
    fixture.mutate(|pass| {
        pass.exploration.cancel();
        Ok(())
    });
    assert!(
        fixture
            .store
            .update_explore(&"review".into(), &old.instance, |pass| pass
                .begin_dispatch(&id, &agent())
                .map_err(|e| e.to_string()))
            .is_err()
    );
    let new = fixture.request(None, None);
    assert!(
        fixture
            .store
            .update_explore(&"review".into(), &old.instance, |pass| pass
                .post(&old)
                .map_err(|e| e.to_string()))
            .is_err()
    );
    let saved = fixture
        .store
        .load_explore(&"review".into(), &old.instance)
        .unwrap()
        .unwrap();
    assert_eq!(saved.exploration.pending_request(), Some(&new));
    assert_eq!(saved.exploration.retry_request(), Some(&new));
}

#[test]
fn editor_sequence_conflict_and_wrong_review_keep_original_bytes() {
    let fixture = Investigation::new();
    let view = fixture.view(1, "Original text");
    fixture
        .store
        .save_explore_view(&"review".into(), &view)
        .unwrap();
    assert!(
        fixture
            .store
            .save_explore_view(&"review".into(), &fixture.view(1, "Conflicting text"))
            .is_err()
    );
    assert!(
        fixture
            .store
            .save_explore_view(&"different-review".into(), &view)
            .is_err()
    );
    assert_eq!(
        fixture
            .store
            .load_explore_view(&"review".into(), &view.instance)
            .unwrap()
            .unwrap(),
        view
    );
}
