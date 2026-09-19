use super::*;
use review_explore::{ExplorePage, ExplorePass, ExploreViewState, ViewSave};

#[path = "explore_concurrent.tests.rs"]
mod concurrent;

fn pass(fixture: &ExploreUi, request: &TurnRequest) -> ExplorePass {
    let mut exploration = review_explore::Exploration::new(fixture.comparison.clone());
    exploration.instance.clone_from(&request.instance);
    let mut pass = ExplorePass::new(exploration);
    pass.post(request).unwrap();
    pass.exploration
        .submit(fixture.response(request, 1))
        .unwrap();
    pass
}

fn restore(fixture: &mut ExploreUi, pass: &ExplorePass, view: Option<ViewSave>) -> Vec<Action> {
    // Serialization removes comparison buffers, preserving only history/source locators.
    let pass = serde_json::from_slice(&serde_json::to_vec(pass).unwrap()).unwrap();
    fixture.app.publish(ui_events::ExploreRestored {
        result: Ok(Some(Arc::new(pass))),
        view,
        passes: vec![],
        historical: false,
        storage_error: None,
    })
}

fn saved(actions: Vec<Action>) -> ViewSave {
    actions
        .into_iter()
        .find_map(|action| match action {
            Action::Explore(Command::SaveView(view)) => Some(*view),
            _ => None,
        })
        .expect("automatic view save")
}

fn no_post(actions: &[Action]) {
    assert!(!actions.iter().any(|action| matches!(
        action,
        Action::Explore(Command::Turn(_) | Command::Implement(_)) | Action::SetReviewed { .. }
    )));
}

#[test]
fn previous_pass_remains_accessible_when_the_current_pass_has_no_agent_response() {
    let (mut fixture, kickoff) = ExploreUi::new();
    let mut exploration = review_explore::Exploration::new(fixture.comparison.clone());
    exploration.instance.clone_from(&kickoff.instance);
    let mut pass = ExplorePass::new(exploration);
    pass.post(&kickoff).unwrap();
    let actions = fixture.app.publish(ui_events::ExploreRestored {
        result: Ok(Some(Arc::new(pass))),
        view: None,
        passes: vec!["older-pass".into(), kickoff.instance],
        historical: false,
        storage_error: None,
    });
    no_post(&actions);
    assert!(fixture.text().contains("[Previous pass]"));
    assert!(
        fixture
            .click_actions("[Previous pass]")
            .iter()
            .any(|action| {
                matches!(action, Action::Explore(Command::OpenPass(id)) if id == "older-pass")
            })
    );
}

#[test]
fn restore_preserves_choice_comment_cursor_and_does_not_send_or_mark_files() {
    let (mut fixture, request) = ExploreUi::new();
    let pass = pass(&fixture, &request);
    let actions = restore(&mut fixture, &pass, None);
    no_post(&actions);
    fixture.app.update(UserInput::Key(Key::Down));
    fixture.app.update(UserInput::Key(Key::Tab));
    fixture.app.update(UserInput::Key(Key::Tab));
    fixture.app.update(UserInput::Paste("before after".into()));
    let view = saved(fixture.app.update(UserInput::Key(Key::Left)));
    assert_eq!(view.state.turns[0].choice, 1);
    let actions = restore(&mut fixture, &pass, Some(view.clone()));
    no_post(&actions);
    let edited = saved(fixture.app.update(UserInput::Key(Key::Char('X'))));
    assert!(
        edited.sequence > view.sequence,
        "autosaves continue across reopening"
    );
    assert_eq!(edited.state.drafts[0].1.editor.text, "before afteXr");
    assert_eq!(edited.state.turns[0].choice, 1);
    let submit = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    assert_eq!(
        submit.answer.unwrap().option.unwrap().id,
        pass.exploration.questions[0].alternatives[1].id
    );
}

#[test]
fn storage_failure_keeps_text_and_never_shows_an_unsaved_answer_as_posted() {
    let (mut fixture, request) = ExploreUi::new();
    let pass = pass(&fixture, &request);
    restore(&mut fixture, &pass, None);
    fixture.app.update(UserInput::Key(Key::Tab));
    fixture.app.update(UserInput::Key(Key::Tab));
    fixture
        .app
        .update(UserInput::Paste("Comment stays here".into()));
    let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    assert!(!fixture.text().contains("You:"));
    fixture.app.publish(ui_events::ExplorePosted {
        request,
        result: Err("Disk full".into()),
    });
    let text = fixture.text();
    assert!(
        text.contains("Disk full") && text.contains("Comment stays here"),
        "{text}"
    );
    assert!(!text.contains("You:"));
}

#[test]
fn separate_conclusions_restore_independent_editors_and_old_conclusion_cannot_implement() {
    let (mut fixture, request) = ExploreUi::new();
    let mut pass = pass(&fixture, &request);
    let mut conclusions = Vec::new();
    for version in 2..=3 {
        let q = pass.exploration.questions.last().cloned();
        let request = pass
            .exploration
            .request(
                Some(review_explore::AnswerInput {
                    text: "Conclude this part".into(),
                    ..Default::default()
                }),
                q.as_ref(),
            )
            .unwrap();
        let mut update = fixture.response(&request, version);
        update.next = None;
        update.topics.clear();
        update.conclusion = Some(review_explore::Conclusion {
            summary: format!("Summary {version}"),
            to_be_implemented: format!("Generated tasks {version}"),
            future_work: "Future only".into(),
        });
        pass.exploration.submit(update).unwrap();
        conclusions.push(request.request.clone());
        if version == 2 {
            let request = pass
                .exploration
                .request(
                    Some(review_explore::AnswerInput {
                        text: "Follow up".into(),
                        ..Default::default()
                    }),
                    None,
                )
                .unwrap();
            let mut update = fixture.response(&request, 4);
            update.interpretation = None;
            pass.exploration.submit(update).unwrap();
        }
    }
    let state = ExploreViewState {
        page: ExplorePage::Conclusion(conclusions[0].clone()),
        tasks: conclusions
            .iter()
            .enumerate()
            .map(|(index, id)| {
                (
                    id.clone(),
                    review_types::TextEditorState {
                        text: format!("Only edited tasks {index}"),
                        ..Default::default()
                    },
                )
            })
            .collect(),
        drafts: conclusions
            .iter()
            .enumerate()
            .map(|(index, id)| {
                (
                    ExplorePage::Conclusion(id.clone()),
                    review_explore::ExploreDraft {
                        editor: review_types::TextEditorState {
                            text: format!("Independent reply {index}"),
                            ..Default::default()
                        },
                        correction: None,
                    },
                )
            })
            .collect(),
        replies: conclusions.iter().map(|id| (id.clone(), true)).collect(),
        ..Default::default()
    };
    restore(
        &mut fixture,
        &pass,
        Some(ViewSave {
            review_unit: pass.exploration.comparison.checkpoint.review_unit.clone(),
            instance: pass.exploration.instance.clone(),
            sequence: 1,
            state,
        }),
    );
    let text = fixture.text();
    assert!(
        text.contains("Only edited tasks 0") && text.contains("Independent reply 0"),
        "{text}"
    );
    assert!(!text.contains("[Implement]"));
    fixture.click("[Conclusion]");
    let text = fixture.text();
    assert!(
        text.contains("Only edited tasks 1") && text.contains("Independent reply 1"),
        "{text}"
    );
    assert!(text.contains("[Implement]"));
}

#[test]
fn unavailable_restored_evidence_keeps_the_question_and_other_references_usable() {
    let (mut fixture, request) = ExploreUi::new();
    let pass = pass(&fixture, &request);
    std::fs::remove_file(fixture.files.root().join("policy.rs")).unwrap();
    restore(&mut fixture, &pass, None);
    let text = fixture.text();
    assert!(
        text.contains("unavailable") && text.contains("Question 1/1"),
        "{text}"
    );
    assert!(text.contains("Keep resolved"));
}

#[test]
fn restored_evidence_position_height_and_focus_survive_reflow_without_selecting_an_answer() {
    let (mut fixture, request) = ExploreUi::new();
    let pass = pass(&fixture, &request);
    let mut state = ExploreViewState {
        page: ExplorePage::Question(0),
        focus: review_explore::EditorFocus::Evidence,
        heights: vec![((0, 0), 15)],
        code: vec![review_explore::EvidencePosition {
            turn: 0,
            reference: 0,
            scroll: 2,
            cursor: 2,
            column: 0,
        }],
        turns: vec![review_explore::QuestionReading {
            choice: 1,
            ..Default::default()
        }],
        ..Default::default()
    };
    let view = ViewSave {
        instance: pass.exploration.instance.clone(),
        review_unit: pass.exploration.comparison.checkpoint.review_unit.clone(),
        sequence: 1,
        state: state.clone(),
    };
    no_post(&restore(&mut fixture, &pass, Some(view)));
    assert_eq!(fixture.app.focus, ui_events::ReviewPane::Detail);
    let sizes = fixture.inline_sizes();
    assert_eq!(sizes[0].1.height + 2, 15);
    fixture.app.publish(ui_events::ExploreViewports(sizes));
    let actions = fixture.app.update(UserInput::Key(Key::Up));
    no_post(&actions);
    let saved = saved(actions);
    assert_eq!(
        saved.state.turns[0].choice, 1,
        "Up navigates restored evidence, not an answer choice"
    );
    assert_eq!(saved.state.code[0].cursor, 1);
    state = saved.state;
    fixture.app.update(UserInput::Resize {
        width: 90,
        height: 40,
    });
    assert_eq!(fixture.inline_sizes()[0].1.height + 2, 15);
    assert_eq!(state.heights, vec![((0, 0), 15)]);
}

#[test]
fn post_ack_only_consumes_its_original_editor_when_history_is_opened_while_saving() {
    let (mut fixture, request) = ExploreUi::new();
    let mut pass = pass(&fixture, &request);
    let question = pass.exploration.questions[0].clone();
    let request = pass
        .exploration
        .request(
            Some(review_explore::AnswerInput {
                text: "Context".into(),
                ..Default::default()
            }),
            Some(&question),
        )
        .unwrap();
    pass.exploration
        .submit(fixture.response(&request, 2))
        .unwrap();
    let view = ViewSave {
        instance: pass.exploration.instance.clone(),
        review_unit: pass.exploration.comparison.checkpoint.review_unit.clone(),
        sequence: 2,
        state: ExploreViewState {
            page: ExplorePage::Question(1),
            editing: true,
            drafts: (0..2)
                .map(|index| {
                    (
                        ExplorePage::Question(index),
                        review_explore::ExploreDraft {
                            editor: review_types::TextEditorState {
                                text: "Same text, different owner".into(),
                                ..Default::default()
                            },
                            correction: None,
                        },
                    )
                })
                .collect(),
            ..Default::default()
        },
    };
    restore(&mut fixture, &pass, Some(view));
    let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    fixture.click("[Previous]");
    pass.post(&request).unwrap();
    let actions = fixture.app.publish(ui_events::ExplorePosted {
        request,
        result: Ok(Arc::new(pass)),
    });
    let view = saved(actions);
    assert_eq!(view.state.page, ExplorePage::Question(0));
    assert_eq!(
        view.state
            .drafts
            .iter()
            .find(|(page, _)| *page == ExplorePage::Question(0))
            .unwrap()
            .1
            .editor
            .text,
        "Same text, different owner"
    );
    assert!(!view.state.drafts.iter().any(|(page, draft)| *page == ExplorePage::Question(1) && !draft.editor.text.is_empty()));
}
