use super::*;
use review_explore::{ConclusionSubmission, ImplementationRequest};

fn finish(fixture: &mut ExploreUi, request: &TurnRequest) -> InterviewUpdate {
    let update = ConclusionSubmission {
        instance: request.instance.clone(),
        review: "runtime-access".into(),
        request: request.request.clone(),
        checkpoint: request.checkpoint.clone(),
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
        Action::Explore(Command::Implement(_)) | Action::SetReviewed { .. } | Action::Thread(_)
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
    let third = implementation(fixture.click_actions(" Implement "));
    assert_ne!(third.delivery, second.delivery);
    assert_eq!(third.text, second.text);
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
    let mut exploration = review_explore::Exploration::new(fixture.comparison.clone());
    exploration.instance.clone_from(&kickoff.instance);
    let mut pass = review_explore::ExplorePass::new(exploration);
    pass.post(&kickoff).unwrap();
    pass.exploration.submit(conclusion).unwrap();
    pass.completion = Some(review_explore::ReviewCompletion {
        request: kickoff.request.clone(),
        baseline: kickoff.checkpoint.checkpoint.clone(),
        marks: vec![],
        completed: true,
        exclusions_enabled: false,
        summary: review_explore::CoverageSummary::default(),
        unexplored: None,
    });
    pass.binding = Some(
        serde_json::from_value(serde_json::json!({
            "agent": "codex",
            "session": {"source": "native", "agent": "codex", "kind": "id", "value": "conversation"}
        }))
        .unwrap(),
    );
    pass.authorize(&request).unwrap();
    let old = pass.implementations[&request.delivery].clone();
    fixture
        .app
        .publish(ui_events::ExploreImplementationSaved(old.clone()));
    pass.authorize(&request).unwrap();
    let current = pass.implementations[&request.delivery].clone();
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
