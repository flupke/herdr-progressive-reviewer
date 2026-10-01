//! Restoring saved passes after a restart, repairing damaged records and retrying
//! interrupted deliveries, through the session interface over a temporary directory.

use review_explore::{DispatchState, ExplorePage, ExploreViewState};
use ui_events::ExploreProgress;

use super::*;

fn error(result: Result<Response, String>) -> String {
    result.expect_err("the session rejected the submission")
}

fn toast(harness: &Harness) -> String {
    harness.next::<ui_events::ToastRequested>().text
}

#[test]
fn a_missing_pass_rejects_submissions_and_restoring_clears_it() {
    let mut harness = Harness::start();
    let (request, access) = harness.conclude();
    let path = harness.pass_path();
    let retained = path.with_extension("retained");
    std::fs::rename(&path, &retained).unwrap();

    assert!(error(harness.submit(&access, conclusion(&request, CONCLUSION))).contains("missing"));
    let restored = harness.reopen();

    assert!(matches!(restored.result, Ok(None)));
    assert!(toast(&harness).contains("Unreadable Explore state was cleared"));
    assert!(harness.history().passes.is_empty());
    assert!(!path.exists());
    assert!(retained.exists());
}

#[test]
fn an_unreadable_index_is_rebuilt_from_intact_passes_as_history() {
    let mut harness = Harness::start();
    harness.capture();
    let first = harness.request(None);
    harness.turn(&first);
    std::fs::write(
        harness.review_directory().join("index.json"),
        b"invalid index",
    )
    .unwrap();

    let restored = harness.reopen();

    assert!(matches!(restored.result, Ok(Some(_))));
    assert!(restored.historical);
    assert!(toast(&harness).contains("readable history was retained"));
    assert_eq!(harness.history().passes, vec![first.instance]);
}

#[test]
fn a_repaired_history_never_guesses_an_editable_pass_and_a_new_pass_is_editable() {
    let mut harness = Harness::start();
    for _ in 0..2 {
        harness.capture();
        let first = harness.request(None);
        harness.turn(&first);
    }
    std::fs::write(
        harness.review_directory().join("index.json"),
        b"invalid index",
    )
    .unwrap();

    let restored = harness.reopen();
    assert!(restored.historical);
    assert_eq!(harness.history().passes.len(), 2);
    assert!(!harness.history().latest_editable);
    harness.adopt(&restored);
    let blocked = harness.request(None);
    harness
        .session
        .handle(Input::Command(Command::Turn(Box::new(blocked))));
    let posted = harness.next::<ui_events::ExplorePosted>();
    assert!(posted.result.unwrap_err().contains("This pass is history"));

    harness.capture();
    let fresh = harness.request(None);
    harness.turn(&fresh);
    let history = harness.history();
    assert!(history.latest_editable);
    assert_eq!(history.passes.last(), Some(&fresh.instance));
}

#[test]
fn an_invalid_pass_is_removed_and_a_new_pass_can_start() {
    let mut harness = Harness::start();
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    assert!(applied(harness.submit(&access, question(&first, 1))));
    let path = harness.pass_path();
    let mut stored: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let evidence = stored
        .pointer_mut("/value/exploration/conversation/0/update/next/evidence/0")
        .and_then(serde_json::Value::as_object_mut)
        .expect("stored question evidence");
    assert!(evidence.remove("notes").is_some());
    std::fs::write(&path, serde_json::to_vec(&stored).unwrap()).unwrap();

    let restored = harness.reopen();

    assert!(matches!(restored.result, Ok(None)));
    assert!(toast(&harness).contains("Unreadable Explore state was cleared"));
    assert!(harness.history().passes.is_empty());
    assert!(!path.exists());
    harness.capture();
}

#[test]
fn an_unreadable_latest_pass_keeps_the_earlier_interview() {
    let mut harness = Harness::start();
    harness.capture();
    let earlier = harness.request(None);
    harness.turn(&earlier);
    harness.capture();
    let latest = harness.request(None);
    harness.turn(&latest);
    let path = harness.pass_path();
    std::fs::write(&path, b"invalid saved pass").unwrap();

    let restored = harness.reopen();

    let pass = restored.result.unwrap().unwrap();
    assert_eq!(pass.exploration.instance, earlier.instance);
    assert!(restored.historical);
    assert!(!path.exists());
    assert_eq!(harness.history().passes, vec![earlier.instance]);
}

#[test]
fn an_invalid_editor_view_is_removed_without_erasing_the_interview() {
    let mut harness = Harness::start();
    harness.capture();
    let first = harness.request(None);
    harness.turn(&first);
    let view = harness
        .review_directory()
        .join(format!("{}.view.json", first.instance));
    std::fs::write(&view, b"invalid editor view").unwrap();

    let restored = harness.reopen();

    assert!(matches!(restored.result, Ok(Some(_))));
    assert!(restored.view.is_none());
    assert!(restored.storage_error.is_none());
    assert!(toast(&harness).contains("Unreadable Explore editor state was cleared"));
    assert!(!view.exists());
    assert_eq!(harness.history().passes, vec![first.instance]);
}

#[test]
fn an_interrupted_turn_restores_without_a_prompt_and_retry_keeps_its_answer() {
    let mut harness = Harness::start();
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    assert!(applied(harness.submit(&access, question(&first, 1))));
    let (request, obsolete) = harness.answer("Retain this exact comment\nand line break");
    let prompts = harness.agents.prompts().len();

    let restored = harness.reopen();

    assert_eq!(restored.progress, ExploreProgress::Interrupted);
    let pass = restored.result.clone().unwrap().unwrap();
    assert_eq!(
        pass.exploration.answers,
        vec![request.answer.clone().unwrap()]
    );
    assert_eq!(pass.exploration.pending_request(), Some(&request));
    assert_eq!(
        harness.agents.prompts().len(),
        prompts,
        "reopening sent nothing"
    );
    harness.adopt(&restored);
    let retry = harness.exploration.as_mut().unwrap().retry().unwrap();
    assert_eq!(retry, request);
    let renewed = harness.turn(&retry);
    assert_ne!(renewed, obsolete);
    assert!(error(harness.submit(&obsolete, question(&retry, 2))).contains("Obsolete"));
    assert!(applied(harness.submit(&renewed, question(&retry, 2))));
    assert_eq!(harness.saved().exploration.answers.len(), 1);
}

#[test]
fn retry_after_uncertain_delivery_sends_the_same_answer_once_more() {
    let mut harness = Harness::start();
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    assert!(applied(harness.submit(&access, question(&first, 1))));
    let (request, _) = harness.answer("Keep it, delivered or not.");
    // The process stopped after starting the prompt, before saving its outcome.
    harness.damage(|pass| {
        pass.turns.get_mut(&request.request).unwrap().state = DispatchState::Attempting;
    });
    let prompts = harness.agents.prompts().len();

    let restored = harness.reopen();

    assert_eq!(restored.progress, ExploreProgress::DeliveryUncertain);
    assert_eq!(
        harness.agents.prompts().len(),
        prompts,
        "reopening sent nothing"
    );
    harness.adopt(&restored);
    let retry = harness.exploration.as_mut().unwrap().retry().unwrap();
    harness
        .session
        .handle(Input::Command(Command::Retry(Box::new(retry.clone()))));
    assert!(harness.next::<ui_events::ExplorePosted>().result.is_ok());
    let prompt = harness.delivered_prompt();
    assert!(prompt.contains(&format!("Answer ID: {}\n", request.answer.unwrap().id)));
    let renewed = prompt
        .lines()
        .find_map(|line| line.strip_prefix("Explore review access: "))
        .unwrap()
        .to_owned();
    assert!(applied(harness.submit(&renewed, question(&retry, 2))));
    let saved = harness.saved();
    assert_eq!(saved.exploration.answers.len(), 1);
    assert_eq!(saved.turns[&retry.request].state, DispatchState::Delivered);
}

#[test]
fn a_saved_view_survives_restart_and_the_old_access_is_rejected() {
    let mut harness = Harness::start();
    let (request, access) = harness.conclude();
    let id = request.request.clone();
    let mut view = ViewSave {
        review_unit: harness.unit.clone(),
        instance: request.instance.clone(),
        sequence: 2,
        state: ExploreViewState {
            page: ExplorePage::Conclusion(id.clone()),
            tasks: std::collections::BTreeMap::from([(
                id.clone(),
                review_types::TextEditorState {
                    text: "Only the edited task".into(),
                    ..Default::default()
                },
            )]),
            drafts: vec![(
                ExplorePage::Conclusion(id.clone()),
                review_types::TextEditorState {
                    text: "Independent unposted question".into(),
                    ..Default::default()
                },
            )],
            ..Default::default()
        },
    };
    harness
        .session
        .handle(Input::Command(Command::SaveView(Box::new(view.clone()))));
    let prompts = harness.agents.prompts().len();

    assert_eq!(harness.reopen().view, Some(view.clone()));
    let accepted = conclusion(&request, CONCLUSION);
    assert!(error(harness.submit(&access, accepted)).contains("Obsolete Explore access"));
    view.sequence += 1;
    view.state.tasks.get_mut(&id).unwrap().text = "Edited after reopening".into();
    harness
        .session
        .handle(Input::Command(Command::SaveView(Box::new(view.clone()))));
    let mut stale = view.clone();
    stale.sequence -= 1;
    harness
        .session
        .handle(Input::Command(Command::SaveView(Box::new(stale))));

    assert_eq!(harness.reopen().view, Some(view));
    assert_eq!(
        harness.agents.prompts().len(),
        prompts,
        "reopening sent nothing"
    );
}

#[test]
fn implementation_delivery_states_survive_restart_without_replaying() {
    let mut harness = Harness::start();
    harness.conclude();
    let request = harness
        .exploration()
        .implementation("Exactly this authorized text".into())
        .unwrap();
    harness.damage(|pass| {
        pass.authorize(&request).unwrap();
    });
    let prompts = harness.agents.prompts().len();
    for state in [
        DispatchState::Queued,
        DispatchState::Attempting,
        DispatchState::Delivered,
    ] {
        harness.damage(|pass| {
            pass.implementations
                .get_mut(&request.delivery)
                .unwrap()
                .state = state.clone();
        });

        let restored = harness.reopen().result.unwrap().unwrap();

        let delivery = &restored.implementations[&request.delivery];
        assert_eq!(delivery.request.text, "Exactly this authorized text");
        assert_eq!(delivery.state, state);
        assert_eq!(
            harness.agents.prompts().len(),
            prompts,
            "reopening sent nothing"
        );
    }
}

#[test]
fn a_lost_acknowledgement_is_saved_and_an_identical_retry_does_not_append() {
    let mut harness = Harness::start();
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    assert!(applied(harness.submit(&access, question(&first, 1))));
    let (request, access) = harness.answer("Keep it.");
    let accepted = || conclusion(&request, "Check Files next");
    assert!(
        error(harness.submit_acknowledged(&access, accepted(), false))
            .contains("retry the identical payload")
    );
    let recorded = harness.saved().exploration.conversation;
    assert_eq!(recorded.len(), 2);
    let prompts = harness.agents.prompts().len();

    let restored = harness.reopen();

    assert_eq!(
        restored
            .result
            .clone()
            .unwrap()
            .unwrap()
            .exploration
            .conversation,
        recorded
    );
    assert_eq!(
        harness.agents.prompts().len(),
        prompts,
        "reopening sent nothing"
    );
    harness.adopt(&restored);
    let follow_up = harness
        .exploration
        .as_mut()
        .unwrap()
        .request(
            Some(AnswerInput {
                text: "Explain the agreed scope".into(),
                ..AnswerInput::default()
            }),
            None,
        )
        .unwrap();
    let renewed = harness.turn(&follow_up);
    assert!(!applied(harness.submit(&renewed, accepted())));
    let conflicting = conclusion(&request, "Rewritten history");
    error(harness.submit(&renewed, conflicting));
    let pass = harness.saved();
    assert_eq!(pass.exploration.conversation, recorded);
    assert_eq!(pass.exploration.pending_request(), Some(&follow_up));
}

#[test]
fn a_late_failure_of_an_old_attempt_cannot_fail_the_retried_turn() {
    let mut harness = Harness::start();
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    assert!(applied(harness.submit(&access, question(&first, 1))));
    let (request, _) = harness.answer("Keep it.");
    let old_attempt = harness.saved().turns[&request.request].attempt.clone();
    harness.session.handle(Input::Command(Command::Cancel));
    let exploration = harness.exploration.as_mut().unwrap();
    exploration.cancel();
    let retry = exploration.retry().unwrap();
    let renewed = harness.turn(&retry);

    harness.session.handle(Input::PromptFinished {
        event: Box::new(ui_events::ExploreFinished {
            instance: request.instance.clone(),
            request: request.request.clone(),
            result: Err("Late cancellation".into()),
        }),
        attempt: old_attempt,
    });

    assert!(applied(
        harness.submit(&renewed, conclusion(&request, "Done."))
    ));
    assert_eq!(harness.saved().exploration.answers.len(), 1);
}

fn agent_in(conversation: Option<&str>) -> Agent {
    Agent {
        agent_session: conversation.map(|value| AgentSession {
            value: value.into(),
            ..agent().agent_session.unwrap()
        }),
        ..agent()
    }
}

#[test]
fn a_restored_pass_prompts_the_conversation_now_running_in_the_pane() {
    let mut harness = Harness::start();
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    assert!(applied(harness.submit(&access, question(&first, 1))));
    let restored = harness.reopen();
    harness.adopt(&restored);
    let replacement = agent_in(Some("different-conversation"));
    harness.agents.upsert_agent(replacement.clone());

    let (request, access) = harness.answer("Exact posted context");

    assert!(
        harness
            .saved()
            .last_agent_session
            .unwrap()
            .matches(&replacement)
    );
    assert!(applied(harness.submit(&access, question(&request, 2))));
    assert_eq!(harness.saved().exploration.answers.len(), 1);
}

#[test]
fn a_pass_without_a_known_conversation_prompts_the_selected_agent_after_restore() {
    let mut harness = Harness::start();
    harness.agents.upsert_agent(agent_in(None));
    harness.capture();
    let first = harness.request(None);
    harness.turn(&first);
    assert!(harness.saved().last_agent_session.is_none());
    let restored = harness.reopen();
    harness.adopt(&restored);
    let identified = agent_in(Some("cannot-prove-this-is-original"));
    harness.agents.upsert_agent(identified.clone());
    let exploration = harness.exploration.as_mut().unwrap();
    let retry = exploration.retry().unwrap();

    harness.turn(&retry);

    assert!(
        harness
            .saved()
            .last_agent_session
            .unwrap()
            .matches(&identified)
    );
}

#[test]
fn an_explicit_retry_adopts_the_agent_now_running_in_the_same_pane() {
    let mut harness = Harness::start();
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    assert!(applied(harness.submit(&access, question(&first, 1))));
    harness.agents.remove_agent(&PaneId(PANE.into()));
    let request = harness.request(Some(AnswerInput {
        option: Some("keep".into()),
        text: "Keep the existing policy".into(),
        ..AnswerInput::default()
    }));
    harness
        .session
        .handle(Input::Command(Command::Turn(Box::new(request.clone()))));
    assert!(harness.next::<ui_events::ExploreFinished>().result.is_err());
    let restored = harness.reopen();
    assert_eq!(restored.progress, ExploreProgress::Interrupted);
    harness.adopt(&restored);
    let refreshed = agent_in(Some("refreshed-conversation"));
    harness.agents.upsert_agent(refreshed.clone());
    let retry = harness.exploration.as_mut().unwrap().retry().unwrap();
    assert_eq!(retry.answer, request.answer);

    harness
        .session
        .handle(Input::Command(Command::Retry(Box::new(retry.clone()))));
    assert!(harness.next::<ui_events::ExplorePosted>().result.is_ok());
    let prompt = harness.delivered_prompt();
    let renewed = prompt
        .lines()
        .find_map(|line| line.strip_prefix("Explore review access: "))
        .unwrap()
        .to_owned();

    assert_ne!(renewed, access);
    assert!(
        harness
            .saved()
            .last_agent_session
            .unwrap()
            .matches(&refreshed)
    );
    assert!(error(harness.submit(&access, question(&retry, 2))).contains("Obsolete"));
    assert!(applied(harness.submit(&renewed, question(&retry, 2))));
}

#[test]
fn a_restored_conclusion_is_implemented_by_the_selected_agent_without_a_conversation() {
    let mut harness = Harness::start();
    harness.conclude();
    harness.agents.upsert_agent(agent_in(None));
    let restored = harness.reopen();
    harness.adopt(&restored);
    let request = harness
        .exploration()
        .implementation("Implement the edited task list.".into())
        .unwrap();

    harness
        .session
        .handle(Input::Command(Command::Implement(request.clone())));

    let finished = harness.next::<ui_events::ExploreImplementationFinished>();
    assert_eq!(finished.request, request);
    assert_eq!(finished.state, DispatchState::Delivered);
    let prompts = harness.agents.prompts();
    assert!(
        prompts
            .last()
            .unwrap()
            .text
            .contains("Implement the edited task list.")
    );
}

#[test]
fn an_explicit_retry_without_a_conversation_binds_the_foreground_process() {
    let mut harness = Harness::start();
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    assert!(applied(harness.submit(&access, question(&first, 1))));
    harness.agents.remove_agent(&PaneId(PANE.into()));
    let request = harness.request(Some(AnswerInput {
        option: Some("keep".into()),
        text: "Keep the existing policy".into(),
        ..AnswerInput::default()
    }));
    harness
        .session
        .handle(Input::Command(Command::Turn(Box::new(request))));
    assert!(harness.next::<ui_events::ExploreFinished>().result.is_err());
    let restored = harness.reopen();
    harness.adopt(&restored);
    harness.agents.upsert_agent(agent_in(None));
    harness.agents.set_process_group(&PaneId(PANE.into()), 42);
    let retry = harness.exploration.as_mut().unwrap().retry().unwrap();

    harness
        .session
        .handle(Input::Command(Command::Retry(Box::new(retry.clone()))));
    assert!(harness.next::<ui_events::ExplorePosted>().result.is_ok());
    let prompt = harness.delivered_prompt();
    let renewed = prompt
        .lines()
        .find_map(|line| line.strip_prefix("Explore review access: "))
        .unwrap()
        .to_owned();

    assert!(harness.saved().last_agent_session.is_none());
    assert!(error(harness.submit(&access, question(&retry, 2))).contains("Obsolete"));
    assert!(applied(harness.submit(&renewed, question(&retry, 2))));
    assert!(harness.saved().last_agent_session.is_none());
    let identified = agent_in(Some("eventual-conversation"));
    harness.agents.upsert_agent(identified.clone());
    assert!(!applied(harness.submit(&renewed, question(&retry, 2))));
    assert!(
        harness
            .saved()
            .last_agent_session
            .unwrap()
            .matches(&identified)
    );
}
