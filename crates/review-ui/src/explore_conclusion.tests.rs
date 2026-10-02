use super::*;
use crate::RepositoryAction;
use review_explore::{ConclusionSubmission, ImplementationRequest};

fn finish(fixture: &mut ExploreUi, request: &TurnRequest) -> InterviewUpdate {
    let update = ConclusionSubmission {
        instance: request.instance.clone(),
        review: "runtime-access".into(),
        request: request.request.clone(),
        checkpoint: request.checkpoint.clone(),
        reviewed: Vec::new(),
        reopened: Vec::new(),
        interpretation: request
            .answer
            .as_ref()
            .filter(|answer| answer.question.is_some())
            .map(|answer| Interpretation {
                answer: answer.id.clone(),
                status: TopicStatus::Accepted,
                recap: "Recorded: keep resolved.".into(),
                follow_ups: vec![],
            }),
        conclusion: review_explore::Conclusion {
            summary: "Keep the agreed resolution policy.".into(),
            to_be_implemented: "1. Preserve resolved state.\n2. Add regression coverage.".into(),
            future_work: "Consider cross-repository notifications later.".into(),
        },
    }
    .into_update();
    assert!(submit(fixture, update.clone()));
    update
}

fn submit(fixture: &mut ExploreUi, update: InterviewUpdate) -> bool {
    let (response, received) = std::sync::mpsc::channel();
    let actions = fixture
        .app
        .publish(ui_events::ExploreSubmission { update, response });
    assert!(!actions.iter().any(|action| matches!(
        action,
        Action::Explore(Command::Implement(_))
            | Action::Repository(RepositoryAction::SetReviewed { .. })
            | Action::Thread(_)
    )));
    received.recv().unwrap().unwrap()
}

fn implementation(actions: Vec<Action>) -> ImplementationRequest {
    actions
        .into_iter()
        .find_map(|action| match action {
            Action::Explore(Command::Implement(request)) => Some(request),
            _ => None,
        })
        .expect("explicit implementation request")
}

fn authorized_round(
    fixture: &ExploreUi,
    kickoff: &TurnRequest,
    conclusion: InterviewUpdate,
    request: &ImplementationRequest,
) -> review_explore::ExploreRound {
    let mut exploration = review_explore::Exploration::new(fixture.comparison.clone());
    exploration.instance.clone_from(&kickoff.instance);
    let mut round = review_explore::ExploreRound::new(exploration);
    round.post(kickoff).unwrap();
    round.exploration.submit(conclusion).unwrap();
    round.completion = Some(review_explore::ReviewCompletion {
        request: kickoff.request.clone(),
        baseline: kickoff.checkpoint.checkpoint.clone(),
    });
    round.last_agent_session = Some(
        serde_json::from_value(serde_json::json!({
            "agent": "codex",
            "session": {"source": "native", "agent": "codex", "kind": "id", "value": "conversation"}
        }))
        .unwrap(),
    );
    round.authorize(request).unwrap();
    round
}

/// Restores a saved request as if the app restarted while it was in `state`.
fn restart_with_delivery(
    fixture: &mut ExploreUi,
    kickoff: &TurnRequest,
    state: review_explore::DispatchState,
) -> ImplementationRequest {
    let conclusion = finish(fixture, kickoff);
    let request = implementation(fixture.click_actions(" Implement "));
    let mut round = authorized_round(fixture, kickoff, conclusion, &request);
    round
        .implementations
        .get_mut(&request.delivery)
        .unwrap()
        .state = state;
    fixture.app.publish(ui_events::ExploreRestored {
        result: Ok(Some(std::sync::Arc::new(round))),
        view: None,
        historical: false,
        storage_error: None,
        progress: ui_events::ExploreProgress::Ready,
    });
    request
}

#[test]
fn sending_a_saved_request_shows_its_text_instead_of_later_edits() {
    let (mut fixture, kickoff) = ExploreUi::new();
    let saved = restart_with_delivery(
        &mut fixture,
        &kickoff,
        review_explore::DispatchState::Queued,
    );
    assert!(
        fixture
            .text()
            .contains("Saved implementation request is paused")
    );
    fixture.click("To be implemented");
    fixture
        .app
        .update(UserInput::Paste("Unsent edit:\n".into()));
    let sent = implementation(fixture.click_actions(" Send saved implementation request "));
    assert_eq!(sent.text, saved.text);
    fixture
        .app
        .publish(ui_events::ExploreImplementationFinished {
            request: sent,
            attempt: None,
            state: review_explore::DispatchState::Delivered,
        });
    let text = fixture.text();
    assert!(text.contains("To be implemented · read-only"), "{text}");
    assert!(text.contains("1. Preserve resolved state."), "{text}");
    assert!(!text.contains("Unsent edit"), "{text}");
}

#[test]
fn a_sent_request_restored_after_restart_shows_its_text_read_only() {
    let (mut fixture, kickoff) = ExploreUi::new();
    restart_with_delivery(
        &mut fixture,
        &kickoff,
        review_explore::DispatchState::Delivered,
    );
    fixture.click("To be implemented");
    fixture.app.update(UserInput::Paste("Blocked edit".into()));
    let text = fixture.text();
    assert!(
        text.contains("Implementation request sent to the agent."),
        "{text}"
    );
    assert!(text.contains("To be implemented · read-only"), "{text}");
    assert!(text.contains("1. Preserve resolved state."), "{text}");
    assert!(!text.contains("Blocked edit"), "{text}");
}

#[test]
fn unknown_delivery_is_read_only_until_a_new_request_is_started() {
    let (mut fixture, kickoff) = ExploreUi::new();
    restart_with_delivery(
        &mut fixture,
        &kickoff,
        review_explore::DispatchState::Attempting,
    );
    let text = fixture.text();
    assert!(text.contains("Delivery outcome unknown"), "{text}");
    assert!(text.contains("To be implemented · read-only"), "{text}");
    assert!(text.contains("1. Preserve resolved state."), "{text}");
    fixture.click("To be implemented");
    fixture.app.update(UserInput::Paste("Blocked edit".into()));
    assert!(!fixture.text().contains("Blocked edit"));
    assert!(
        !fixture
            .click_actions(" New implementation request ")
            .iter()
            .any(|action| matches!(action, Action::Explore(Command::Implement(_))))
    );
    fixture
        .app
        .update(UserInput::Paste("Resent scope:\n".into()));
    let text = fixture.text();
    assert!(!text.contains("read-only"), "{text}");
    let resent = implementation(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    assert!(
        resent
            .text
            .starts_with("Resent scope:\n1. Preserve resolved state.")
    );
}

#[test]
fn conclusion_has_its_own_page_and_sends_only_the_edited_tasks_once() {
    let (mut fixture, request) = ExploreUi::new();
    fixture.respond(&request, 1);
    let answer = ExploreUi::request(fixture.app.update(UserInput::Key(Key::Enter)));
    let update = finish(&mut fixture, &answer);
    let text = fixture.text();
    assert!(
        text.contains("Conclusion") && text.contains("Summary") && text.contains("Future work"),
        "{text}"
    );
    assert!(!text.contains("Question 1:") && !text.contains("Evidence 1/"));
    fixture.click("To be implemented");
    for _ in "1. Preserve resolved state.\n2. Add regression coverage.".chars() {
        fixture.app.update(UserInput::Key(Key::Delete));
    }
    fixture.app.update(UserInput::Paste(
        "Only this edited task — keep exact.\nWith a second line.".into(),
    ));
    fixture.click("[Previous]");
    assert!(fixture.text().contains("Recorded: keep resolved."));
    assert!(!fixture.text().contains("Future work"));
    assert!(!submit(&mut fixture, update));
    assert!(
        fixture.text().contains("Question 1/1"),
        "duplicate must not navigate"
    );
    fixture.click("[Conclusion]");
    let request = implementation(fixture.click_actions(" Implement "));
    assert_eq!(
        request.text,
        "Only this edited task — keep exact.\nWith a second line."
    );
    assert_eq!(request.conclusion, answer.request);
    assert!(
        !fixture
            .app
            .update(UserInput::Key(Key::ControlEnter))
            .iter()
            .any(|action| matches!(action, Action::Explore(Command::Implement(_))))
    );
    fixture
        .app
        .publish(ui_events::ExploreImplementationFinished {
            request,
            attempt: None,
            state: review_explore::DispatchState::Delivered,
        });
    assert!(
        fixture
            .text()
            .contains("Implementation request sent to the agent.")
    );
    assert!(!fixture.text().contains(" Implement "));
}

#[test]
fn submitted_instructions_stay_read_only_in_both_editor_keymaps() {
    for keymap in [
        comment_editor::EditorKeymap::Regular,
        comment_editor::EditorKeymap::Vim,
    ] {
        let (mut fixture, kickoff) = ExploreUi::new();
        fixture.app.set_editor_keymap(keymap);
        finish(&mut fixture, &kickoff);
        fixture
            .app
            .update(UserInput::Paste("Exact submitted scope:\n".into()));
        let request = implementation(fixture.app.update(UserInput::Key(Key::ControlEnter)));
        for delivered in [false, true] {
            if delivered {
                fixture
                    .app
                    .publish(ui_events::ExploreImplementationFinished {
                        request: request.clone(),
                        attempt: None,
                        state: review_explore::DispatchState::Delivered,
                    });
            }
            fixture.click("To be implemented");
            for key in [Key::Enter, Key::Char('i'), Key::Delete, Key::Backspace] {
                fixture.app.update(UserInput::Key(key));
            }
            fixture
                .app
                .update(UserInput::Paste("Unsent modification".into()));
            fixture.app.update(UserInput::Key(Key::Tab));
            fixture
                .app
                .update(UserInput::Paste("Another modification".into()));
            assert!(
                !fixture
                    .app
                    .update(UserInput::Key(Key::ControlEnter))
                    .iter()
                    .any(|action| matches!(action, Action::Explore(Command::Implement(_))))
            );
            let text = fixture.text();
            assert!(text.contains("To be implemented · read-only"), "{text}");
            assert!(text.contains("Exact submitted scope:"), "{text}");
            assert!(text.contains("1. Preserve resolved state."), "{text}");
            assert!(!text.contains("modification"), "{text}");
            assert!(!text.contains("F2 ·") && !text.contains("Ctrl-Enter Implement"));
        }
        fixture.click("[Reply]");
        fixture
            .app
            .update(UserInput::Paste("Follow-up question".into()));
        let reply = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
        assert_eq!(reply.answer.unwrap().text, "Follow-up question");
    }
}

#[test]
fn failed_or_cancelled_delivery_keeps_edits_and_ignores_obsolete_acknowledgements() {
    let (mut fixture, request) = ExploreUi::new();
    finish(&mut fixture, &request);
    let first = implementation(fixture.click_actions(" Implement "));
    let actions = fixture.click_actions(" Cancel implementation ");
    assert!(
        actions
            .iter()
            .any(|action| matches!(action, Action::Explore(Command::CancelImplementation)))
    );
    fixture
        .app
        .publish(ui_events::ExploreImplementationFinished {
            request: first.clone(),
            attempt: None,
            state: review_explore::DispatchState::Cancelled,
        });
    assert!(!fixture.text().contains("Implementation request sent"));
    let second = implementation(fixture.click_actions(" Implement "));
    fixture
        .app
        .publish(ui_events::ExploreImplementationFinished {
            request: first,
            attempt: None,
            state: review_explore::DispatchState::Delivered,
        });
    assert!(!fixture.text().contains("Implementation request sent"));
    fixture
        .app
        .publish(ui_events::ExploreImplementationFinished {
            request: second.clone(),
            attempt: None,
            state: review_explore::DispatchState::NotSent("Agent unavailable".into()),
        });
    assert!(fixture.text().contains("Agent unavailable"));
    fixture.click("To be implemented");
    fixture
        .app
        .update(UserInput::Paste("Revised after failure:\n".into()));
    let third = implementation(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    assert_ne!(third.delivery, second.delivery);
    assert_eq!(
        third.text,
        format!("Revised after failure:\n{}", second.text)
    );
}

#[test]
fn initial_conclusion_keeps_the_opening_reply_without_an_opening_page() {
    let (mut fixture, request) = ExploreUi::new();
    let mut update = fixture.response(&request, 1);
    update.next = None;
    update.conclusion = Some(conclusion("No implementation is agreed."));
    submit(&mut fixture, update);
    assert!(!fixture.text().contains(" Implement "));
    assert!(!fixture.text().contains("[Opening]"));
    assert!(fixture.text().contains("The source supports this context."));
    assert!(fixture.text().contains("No implementation is agreed."));
    fixture
        .app
        .update(UserInput::Paste("An explicit task".into()));
    assert_eq!(
        implementation(fixture.app.update(UserInput::Key(Key::ControlEnter))).text,
        "An explicit task"
    );
}

#[test]
fn history_shows_one_link_per_destination_when_the_latest_page_is_a_conclusion() {
    let (mut fixture, request) = ExploreUi::new();
    fixture.respond(&request, 1);
    let answer = ExploreUi::request(fixture.app.update(UserInput::Key(Key::Enter)));
    finish(&mut fixture, &answer);
    fixture.click("[Previous]");
    let text = fixture.text();
    assert!(text.contains("[Conclusion]"), "{text}");
    assert!(
        !text.contains("[Next]") && !text.contains("[Latest]"),
        "{text}"
    );
}

#[test]
fn cancel_racing_with_completed_delivery_reports_that_the_request_was_sent() {
    let (mut fixture, request) = ExploreUi::new();
    finish(&mut fixture, &request);
    let request = implementation(fixture.click_actions(" Implement "));
    fixture.click(" Cancel implementation ");
    fixture
        .app
        .publish(ui_events::ExploreImplementationFinished {
            request,
            attempt: None,
            state: review_explore::DispatchState::Delivered,
        });
    assert!(
        fixture
            .text()
            .contains("Implementation request sent to the agent.")
    );
    assert!(!fixture.text().contains(" Implement "));
}

#[test]
fn a_receipt_from_a_previous_attempt_cannot_finish_the_current_implementation() {
    let (mut fixture, kickoff) = ExploreUi::new();
    let conclusion = finish(&mut fixture, &kickoff);
    let request = implementation(fixture.click_actions(" Implement "));
    let mut round = authorized_round(&fixture, &kickoff, conclusion, &request);
    let old = round.implementations[&request.delivery].clone();
    fixture
        .app
        .publish(ui_events::ExploreImplementationSaved(old.clone()));
    round.authorize(&request).unwrap();
    let current = round.implementations[&request.delivery].clone();
    fixture
        .app
        .publish(ui_events::ExploreImplementationSaved(current.clone()));
    fixture
        .app
        .publish(ui_events::ExploreImplementationFinished {
            request: request.clone(),
            attempt: Some(old.attempt),
            state: review_explore::DispatchState::Cancelled,
        });
    assert!(fixture.text().contains(" Cancel implementation "));
    assert!(!fixture.text().contains(" Implement "));
    fixture
        .app
        .publish(ui_events::ExploreImplementationFinished {
            request,
            attempt: Some(current.attempt),
            state: review_explore::DispatchState::Unknown,
        });
    assert!(fixture.text().contains("Delivery outcome unknown"));
    assert!(!fixture.text().contains(" Implement ") && !fixture.text().contains(" Retry "));
}

#[test]
fn implementation_editor_and_conversation_reply_keep_separate_text_and_focus() {
    let (mut fixture, request) = ExploreUi::new();
    finish(&mut fixture, &request);
    fixture.click("[Reply]");
    fixture.app.update(UserInput::Paste(
        "Unposted question about the conclusion".into(),
    ));
    fixture.app.update(UserInput::Key(Key::Tab));
    fixture.app.update(UserInput::Resize {
        width: 100,
        height: 24,
    });
    for _ in 0..5 {
        fixture.app.update(UserInput::Key(Key::PageUp));
    }
    fixture.click("To be implemented");
    fixture
        .app
        .update(UserInput::Paste("Edited task list:\n".into()));
    assert!(fixture.text().contains("To be implemented"));
    let request = implementation(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    assert_eq!(
        request.text,
        "Edited task list:\n1. Preserve resolved state.\n2. Add regression coverage."
    );
    fixture.app.update(UserInput::Resize {
        width: 140,
        height: 70,
    });
    fixture.click("[Reply]");
    assert!(
        fixture
            .text()
            .contains("Unposted question about the conclusion")
    );
}

#[test]
fn revised_conclusions_retain_each_edited_list_reply_and_draft_in_posting_order() {
    for intervening_question in [false, true] {
        let (mut fixture, request) = ExploreUi::new();
        fixture.app.update(UserInput::Resize {
            width: 140,
            height: 100,
        });
        let first = finish(&mut fixture, &request);
        fixture
            .app
            .update(UserInput::Paste("First edited scope:\n".into()));
        fixture.click("[Reply]");
        fixture
            .app
            .update(UserInput::Paste("Please revise this conclusion".into()));
        let mut request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
        assert_eq!(request.answer.as_ref().unwrap().in_reply_to, first.request);
        fixture
            .app
            .update(UserInput::Paste("Unposted first-conclusion context".into()));
        if intervening_question {
            let mut question = fixture.response(&request, 1);
            question.interpretation = None;
            submit(&mut fixture, question);
            request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::Enter)));
        }
        let second = finish(&mut fixture, &request);
        fixture
            .app
            .update(UserInput::Paste("Second edited scope:\n".into()));
        fixture.click("[Previous]");
        if intervening_question {
            assert!(fixture.text().contains("Question 1/1"));
            fixture.click("[Previous]");
        }
        let text = fixture.text();
        for expected in [
            "Conclusion 1/2",
            "First edited scope:",
            "You: Please revise this conclusion",
            "Unposted first-conclusion context",
        ] {
            assert!(text.contains(expected), "{expected}: {text}");
        }
        assert!(!text.contains(" Implement ") && !text.contains("Second edited scope:"));
        fixture.click("To be implemented");
        assert!(
            !fixture
                .app
                .update(UserInput::Key(Key::ControlEnter))
                .iter()
                .any(|action| matches!(action, Action::Explore(Command::Implement(_))))
        );
        fixture.click("[Conclusion]");
        assert!(fixture.text().contains("Conclusion 2/2"));
        let tasks = implementation(fixture.click_actions(" Implement "));
        assert_eq!(tasks.conclusion, second.request);
        assert!(tasks.text.starts_with("Second edited scope:\n"));
        assert!(!tasks.text.contains("First edited scope") && !tasks.text.contains("context"));
    }
}
