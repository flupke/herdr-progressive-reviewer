use super::*;
use crate::RepositoryAction;
use review_explore::{ExplorePage, ExploreRound, ExploreViewState, ViewSave};

#[path = "explore_concurrent.tests.rs"]
mod concurrent;

pub(super) fn round(fixture: &ExploreUi, request: &TurnRequest) -> ExploreRound {
    let mut exploration = review_explore::Exploration::new(fixture.comparison.clone());
    exploration.instance.clone_from(&request.instance);
    let mut round = ExploreRound::new(exploration);
    round.post(request).unwrap();
    round
        .exploration
        .submit(fixture.response(request, 1))
        .unwrap();
    round
}

pub(super) fn restore(
    fixture: &mut ExploreUi,
    round: &ExploreRound,
    view: Option<ViewSave>,
) -> Vec<Action> {
    // Serialization removes comparison buffers, preserving only history/source locators.
    let round = serde_json::from_slice(&serde_json::to_vec(round).unwrap()).unwrap();
    fixture.app.publish(ui_events::ExploreRestored {
        result: Ok(Some(Arc::new(round))),
        view,
        historical: false,
        storage_error: None,
        progress: ui_events::ExploreProgress::Ready,
        prepared_turns: Vec::new(),
    })
}

pub(super) fn saved(actions: Vec<Action>) -> ViewSave {
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
        Action::Explore(Command::Turn(_) | Command::Implement(_))
            | Action::Repository(RepositoryAction::SetReviewed { .. })
    )));
}

#[test]
fn restore_preserves_choice_comment_cursor_and_does_not_send_or_mark_files() {
    let (mut fixture, request) = ExploreUi::new();
    let round = round(&fixture, &request);
    let actions = restore(&mut fixture, &round, None);
    no_post(&actions);
    fixture.app.update(UserInput::Key(Key::Down));
    fixture.app.update(UserInput::Key(Key::Tab));
    fixture.app.update(UserInput::Key(Key::Tab));
    fixture.app.update(UserInput::Paste("before after".into()));
    let view = saved(fixture.app.update(UserInput::Key(Key::Left)));
    assert_eq!(view.state.turns[0].choice, 1);
    let actions = restore(&mut fixture, &round, Some(view.clone()));
    no_post(&actions);
    let edited = saved(fixture.app.update(UserInput::Key(Key::Char('X'))));
    assert!(
        edited.sequence > view.sequence,
        "autosaves continue across reopening"
    );
    assert_eq!(edited.state.drafts[0].1.text, "before afteXr");
    assert_eq!(edited.state.turns[0].choice, 1);
    let submit = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    assert_eq!(
        submit.answer.unwrap().option.unwrap().id,
        round.exploration.questions[0].alternatives[1].id
    );
}

#[test]
fn restored_question_clamps_a_saved_scroll_past_the_document() {
    let (mut fixture, request) = ExploreUi::new();
    let round = round(&fixture, &request);
    let view = ViewSave {
        instance: round.exploration.instance.clone(),
        review_unit: round.exploration.comparison.checkpoint.review_unit.clone(),
        sequence: 1,
        state: ExploreViewState {
            page: ExplorePage::Question(0),
            scroll: 10_000,
            ..Default::default()
        },
    };
    no_post(&restore(&mut fixture, &round, Some(view)));

    let text = fixture.text();
    assert!(text.contains("Question 1: keep resolved?"), "{text}");
    assert!(!text.contains("[Reply]"), "{text}");
}

#[test]
fn saved_opening_page_restores_to_the_first_question() {
    let (mut fixture, request) = ExploreUi::new();
    let round = round(&fixture, &request);
    let view = ViewSave {
        instance: round.exploration.instance.clone(),
        review_unit: round.exploration.comparison.checkpoint.review_unit.clone(),
        sequence: 1,
        state: ExploreViewState {
            page: ExplorePage::Opening,
            ..Default::default()
        },
    };
    no_post(&restore(&mut fixture, &round, Some(view)));
    let text = fixture.text();
    assert!(text.contains("Question 1: keep resolved?"), "{text}");
    assert!(!text.contains("[Opening]"));
}

#[test]
fn saved_opening_page_restores_to_an_initial_conclusion() {
    let (mut fixture, kickoff) = ExploreUi::new();
    let mut exploration = review_explore::Exploration::new(fixture.comparison.clone());
    exploration.instance.clone_from(&kickoff.instance);
    let mut round = ExploreRound::new(exploration);
    round.post(&kickoff).unwrap();
    let mut update = fixture.response(&kickoff, 1);
    update.next = None;
    update.conclusion = Some(conclusion("No question was needed."));
    round.exploration.submit(update).unwrap();
    let view = ViewSave {
        instance: round.exploration.instance.clone(),
        review_unit: round.exploration.comparison.checkpoint.review_unit.clone(),
        sequence: 1,
        state: ExploreViewState {
            page: ExplorePage::Opening,
            ..Default::default()
        },
    };
    no_post(&restore(&mut fixture, &round, Some(view)));
    let text = fixture.text();
    assert!(text.contains("No question was needed."), "{text}");
    assert!(!text.contains("[Opening]"));
}

#[test]
fn historical_question_offers_reset_after_round_navigation_is_removed() {
    let (mut fixture, kickoff) = ExploreUi::new();
    let round = round(&fixture, &kickoff);
    no_post(&fixture.app.publish(ui_events::ExploreRestored {
        result: Ok(Some(Arc::new(round))),
        view: None,
        historical: true,
        storage_error: None,
        progress: ui_events::ExploreProgress::Ready,
        prepared_turns: Vec::new(),
    }));
    let text = fixture.text();
    assert!(
        text.contains("Earlier round") && text.contains(" Reset "),
        "{text}"
    );
    assert!(!text.contains("[Previous pass]"));
}

#[test]
fn storage_failure_keeps_text_and_never_shows_an_unsaved_answer_as_posted() {
    let (mut fixture, request) = ExploreUi::new();
    let round = round(&fixture, &request);
    restore(&mut fixture, &round, None);
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
    let mut round = round(&fixture, &request);
    let mut conclusions = Vec::new();
    for version in 2..=3 {
        let q = round.exploration.questions.last().cloned();
        let request = round
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
            quiz: Vec::new(),
            quiz_empty_reason: Some("Nothing at whiteboard level.".into()),
        });
        round.exploration.submit(update).unwrap();
        conclusions.push(request.request.clone());
        if version == 2 {
            let request = round
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
            round.exploration.submit(update).unwrap();
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
                    review_types::TextEditorState {
                        text: format!("Independent reply {index}"),
                        ..Default::default()
                    },
                )
            })
            .collect(),
        replies: conclusions.iter().map(|id| (id.clone(), true)).collect(),
        ..Default::default()
    };
    restore(
        &mut fixture,
        &round,
        Some(ViewSave {
            review_unit: round.exploration.comparison.checkpoint.review_unit.clone(),
            instance: round.exploration.instance.clone(),
            sequence: 1,
            state,
        }),
    );
    let text = fixture.text();
    assert!(
        text.contains("Only edited tasks 0") && text.contains("Independent reply 0"),
        "{text}"
    );
    assert!(!text.contains(" Implement "));
    fixture.click("[Conclusion]");
    let text = fixture.text();
    assert!(
        text.contains("Only edited tasks 1") && text.contains("Independent reply 1"),
        "{text}"
    );
    assert!(text.contains(" Implement "), "{text}");
}

#[test]
fn unavailable_restored_evidence_keeps_the_question_and_other_citations_usable() {
    let (mut fixture, request) = ExploreUi::new();
    let round = round(&fixture, &request);
    std::fs::remove_file(fixture.files.root().join("policy.rs")).unwrap();
    restore(&mut fixture, &round, None);
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
    let round = round(&fixture, &request);
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
        instance: round.exploration.instance.clone(),
        review_unit: round.exploration.comparison.checkpoint.review_unit.clone(),
        sequence: 1,
        state: state.clone(),
    };
    no_post(&restore(&mut fixture, &round, Some(view)));
    assert_eq!(
        fixture.app.navigation.focus(),
        ui_events::ReviewPane::Detail
    );
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
    let mut round = round(&fixture, &request);
    let question = round.exploration.questions[0].clone();
    let request = round
        .exploration
        .request(
            Some(review_explore::AnswerInput {
                text: "Context".into(),
                ..Default::default()
            }),
            Some(&question),
        )
        .unwrap();
    round
        .exploration
        .submit(fixture.response(&request, 2))
        .unwrap();
    let view = ViewSave {
        instance: round.exploration.instance.clone(),
        review_unit: round.exploration.comparison.checkpoint.review_unit.clone(),
        sequence: 2,
        state: ExploreViewState {
            page: ExplorePage::Question(1),
            editing: true,
            drafts: (0..2)
                .map(|index| {
                    (
                        ExplorePage::Question(index),
                        review_types::TextEditorState {
                            text: "Same text, different owner".into(),
                            ..Default::default()
                        },
                    )
                })
                .collect(),
            ..Default::default()
        },
    };
    restore(&mut fixture, &round, Some(view));
    let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::ControlEnter)));
    fixture.click("[Previous]");
    round.post(&request).unwrap();
    let actions = fixture.app.publish(ui_events::ExplorePosted {
        request,
        result: Ok(Arc::new(round)),
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
            .text,
        "Same text, different owner"
    );
    assert!(
        !view
            .state
            .drafts
            .iter()
            .any(|(page, draft)| *page == ExplorePage::Question(1) && !draft.text.is_empty())
    );
}

#[test]
fn restored_progress_from_the_session_decides_the_recovery_status() {
    for (progress, status) in [
        (
            ui_events::ExploreProgress::DeliveryUncertain,
            Some("Previous prompt delivery is uncertain."),
        ),
        (
            ui_events::ExploreProgress::Interrupted,
            Some("Interview turn interrupted."),
        ),
        (ui_events::ExploreProgress::Ready, None),
    ] {
        let (mut fixture, request) = ExploreUi::new();
        let round = round(&fixture, &request);
        no_post(&fixture.app.publish(ui_events::ExploreRestored {
            result: Ok(Some(Arc::new(round))),
            view: None,
            historical: false,
            storage_error: None,
            progress,
            prepared_turns: Vec::new(),
        }));
        let text = fixture.text();
        match status {
            Some(status) => assert!(text.contains(status), "{progress:?}: {text}"),
            None => assert!(
                !text.contains("Retry keeps the posted answer"),
                "{progress:?}: {text}"
            ),
        }
    }
}

#[test]
fn cancelling_the_latest_answer_brings_its_question_back_to_answer_again() {
    let (mut fixture, request) = ExploreUi::new();
    let mut round = round(&fixture, &request);
    let question = round.exploration.questions[0].clone();
    let answered = round
        .exploration
        .clone()
        .request(
            Some(review_explore::AnswerInput {
                option: Some("inspect".into()),
                text: "Check the retry path.".into(),
                ..Default::default()
            }),
            Some(&question),
        )
        .unwrap();
    round.post(&answered).unwrap();
    round
        .exploration
        .submit(fixture.response(&answered, 2))
        .unwrap();
    no_post(&restore(&mut fixture, &round, None));
    fixture.click("Your answer");
    fixture
        .app
        .update(UserInput::Paste("Draft for the discarded question".into()));
    assert!(fixture.text().contains("Draft for the discarded question"));
    fixture.app.update(UserInput::Key(Key::Tab));
    fixture.app.update(UserInput::Key(Key::Char('[')));
    assert!(fixture.text().contains("You: Inspect the caller"));

    let actions = fixture.click_actions("Cancel answer");
    let id = answered.answer.as_ref().unwrap().id.clone();
    assert!(actions.iter().any(|action| matches!(
        action,
        Action::Explore(Command::CancelAnswer(answer)) if *answer == id
    )));
    let mut cancelled = round.clone();
    cancelled.cancel_answer(&id).unwrap();
    cancelled.revision += 1;
    fixture.app.publish(ui_events::ExploreAnswerCancelled {
        answer: id.clone(),
        result: Ok(Arc::new(cancelled)),
    });

    let text = fixture.text();
    assert!(text.contains("Question 1: keep resolved?"), "{text}");
    assert!(
        !text.contains("Question 2") && !text.contains("Cancel answer"),
        "{text}"
    );
    assert!(text.contains("› 2. Inspect the caller"), "{text}");
    assert!(text.contains("Check the retry path."), "{text}");
    assert!(!text.contains("Draft for the discarded question"), "{text}");
    let request = ExploreUi::request(fixture.app.update(UserInput::Key(Key::Enter)));
    assert_eq!(request.cancelled, vec![id]);
    let answer = request.answer.unwrap();
    assert_eq!(answer.option.unwrap().id, "inspect");
    assert_eq!(answer.text, "Check the retry path.");
}

#[test]
fn a_turn_that_run_ahead_prepared_says_so_under_the_question_it_asked() {
    let (mut fixture, request) = ExploreUi::new();
    let round = round(&fixture, &request);
    restore(&mut fixture, &round, None);
    assert!(!fixture.text().contains(PREPARED));

    fixture.app.publish(ui_events::ExploreTurnPrepared {
        round: request.instance.clone(),
        request: request.request.clone(),
    });
    assert!(fixture.text().contains(PREPARED), "{}", fixture.text());

    // A reopened reviewer knows it from the saved forks.
    let saved = serde_json::from_slice(&serde_json::to_vec(&round).unwrap()).unwrap();
    fixture.app.publish(ui_events::ExploreRestored {
        result: Ok(Some(Arc::new(saved))),
        view: None,
        historical: false,
        storage_error: None,
        progress: ui_events::ExploreProgress::Ready,
        prepared_turns: vec![request.request.clone()],
    });
    assert!(fixture.text().contains(PREPARED));
}

const PREPARED: &str = "Prepared while you were thinking";
