use super::*;
use review_explore::{
    AnswerInput, Comparison, Conclusion, Exploration, ExplorePass, InterviewUpdate, Question,
    Topic, TopicStatus,
};
use std::sync::Arc;

#[path = "explore_delivery.tests.rs"]
mod delivery;

struct Investigation {
    directory: tempfile::TempDir,
    store: ReviewStore,
    pass: ExplorePass,
}

impl Investigation {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("policy.rs"), "fn policy() {}\n").unwrap();
        let store = ReviewStore::open(directory.path().join("state"), directory.path()).unwrap();
        let comparison = Comparison {
            repository_root: directory.path().to_owned(),
            checkpoint: review_guide::ReviewCheckpoint::new("review", "checkpoint"),
            files: vec![],
            context: vec![],
            diffs: vec![],
            manifest: vec![],
            sources: vec![],
            base: None,
        };
        let pass = ExplorePass::new(Exploration::new(Arc::new(comparison)));
        store.create_explore(pass.clone()).unwrap();
        Self {
            directory,
            store,
            pass,
        }
    }

    fn mutate<T>(
        &mut self,
        f: impl FnOnce(&mut ExplorePass) -> std::result::Result<T, String>,
    ) -> T {
        let (value, pass) = self
            .store
            .update_explore(&"review".into(), &self.pass.exploration.instance, f)
            .unwrap();
        self.pass = pass;
        value
    }

    fn request(
        &mut self,
        input: Option<AnswerInput>,
        question: Option<&Question>,
    ) -> review_explore::TurnRequest {
        let request = self
            .pass
            .exploration
            .clone()
            .request(input, question)
            .unwrap();
        self.mutate(|pass| pass.post(&request).map_err(|e| e.to_string()));
        request
    }

    fn submit(&mut self, update: InterviewUpdate) -> bool {
        self.mutate(|pass| pass.exploration.submit(update).map_err(|e| e.to_string()))
    }

    fn update(request: &review_explore::TurnRequest, number: usize) -> InterviewUpdate {
        serde_json::from_value(serde_json::json!({
            "instance":request.instance, "request":request.request, "checkpoint": request.checkpoint,
            "interpretation":null, "reply":{"text":"Opening or direct answer", "evidence":[]},
            "topics":[{"id":format!("topic{number}"),"title":"Policy", "entries":[],"status":"open"}],
            "next":{"id":format!("q{number}"), "version":1, "topic":format!("topic{number}"),"text":"Which policy?",
                "alternatives":[{"id":"keep","text":"Keep the complete policy", "outcome":"accepted"},{"id":"change","text":"Change policy", "outcome":"needs_follow_up"}],
                "evidence":[{"path":"policy.rs","side":"new","lines":{"first_line":1,"last_line":1}, "relationship":"Defines policy", "decision_relevance":"Determines retention"}]},
            "conclusion":null, "limitations":[], "findings":[]
        })).unwrap()
    }

    fn view(&self, sequence: u64, text: &str) -> ViewSave {
        ViewSave {
            review_unit: self
                .pass
                .exploration
                .comparison
                .checkpoint
                .review_unit
                .clone(),
            instance: self.pass.exploration.instance.clone(),
            sequence,
            state: review_explore::ExploreViewState {
                tasks: std::collections::BTreeMap::from([(
                    "conclusion".into(),
                    review_types::TextEditorState {
                        text: text.into(),
                        row: 2,
                        column: 3,
                        ..Default::default()
                    },
                )]),
                ..Default::default()
            },
        }
    }
}

#[test]
fn adaptive_history_and_separate_conclusions_round_trip_without_source_buffers() {
    let mut fixture = Investigation::new();
    let first = fixture.request(None, None);
    fixture.submit(Investigation::update(&first, 1));
    let question = fixture.pass.exploration.questions[0].clone();
    let answer = fixture.request(
        Some(AnswerInput {
            option: Some("keep".into()),
            text: "Keep the comment exactly: é\nsecond line".into(),
            ..Default::default()
        }),
        Some(&question),
    );
    let mut update = Investigation::update(&answer, 2);
    update.interpretation = Some(review_explore::Interpretation {
        answer: answer.answer.as_ref().unwrap().id.clone(),
        status: TopicStatus::Accepted,
        recap: "Recorded: keep policy".into(),
        follow_ups: vec![],
    });
    update.topics.push(Topic {
        id: "unused".into(),
        title: "Retirable branch".into(),
        ..Default::default()
    });
    fixture.submit(update);
    let correction = fixture.request(
        Some(AnswerInput {
            text: "The unused branch does not apply".into(),
            corrects: Some(answer.answer.as_ref().unwrap().id.clone()),
            ..Default::default()
        }),
        Some(&question),
    );
    let mut update = Investigation::update(&correction, 3);
    update.agenda = vec![serde_json::from_value(serde_json::json!({"topic":"unused","action":"retire","reason":"Reviewer corrected the premise","answer":correction.answer.as_ref().unwrap().id,"evidence":[],"replacement":null,"decision":null})).unwrap()];
    fixture.submit(update);
    for number in 4..=5 {
        let question = fixture.pass.exploration.questions.last().cloned();
        let request = fixture.request(
            Some(AnswerInput {
                text: "Conclude this part".into(),
                ..Default::default()
            }),
            question.as_ref(),
        );
        let mut update = Investigation::update(&request, number);
        update.next = None;
        update.topics.clear();
        update.conclusion = Some(Conclusion {
            summary: "Further human Files inspection is required".into(),
            to_be_implemented: format!("Task {number}"),
            future_work: "Future only".into(),
        });
        fixture.submit(update);
        if number == 4 {
            let request = fixture.request(
                Some(AnswerInput {
                    text: "Follow up on the conclusion".into(),
                    ..Default::default()
                }),
                None,
            );
            fixture.submit(Investigation::update(&request, 6));
        }
    }
    let expected = fixture.pass.clone();
    std::fs::remove_file(fixture.directory.path().join("policy.rs")).unwrap();
    let reopened = ReviewStore::open(
        fixture.directory.path().join("state"),
        fixture.directory.path(),
    )
    .unwrap();
    let restored = reopened
        .load_explore(&"review".into(), &expected.exploration.instance)
        .unwrap()
        .unwrap();
    assert_eq!(restored, expected);
    assert_eq!(
        restored
            .exploration
            .conversation
            .iter()
            .filter(|turn| turn.update.conclusion.is_some())
            .count(),
        2
    );
    assert_eq!(restored.exploration.answers[0], answer.answer.unwrap());
    assert_eq!(
        restored.exploration.topics["unused"].status,
        TopicStatus::Open
    );
    assert_eq!(restored.exploration.interpretations.len(), 1);
}

#[test]
fn accepted_output_survives_lost_ack_and_conflicting_retries_cannot_rewrite_it() {
    let mut fixture = Investigation::new();
    let request = fixture.request(None, None);
    let update = Investigation::update(&request, 1);
    assert!(fixture.submit(update.clone()));
    let revision = fixture.pass.revision;
    assert!(!fixture.submit(update.clone()));
    assert_eq!(fixture.pass.revision, revision);
    let mut conflict = update;
    conflict.next.as_mut().unwrap().text = "Replacement".into();
    let result = fixture.store.update_explore(
        &"review".into(),
        &fixture.pass.exploration.instance,
        |pass| pass.exploration.submit(conflict).map_err(|e| e.to_string()),
    );
    assert!(result.is_err());
    assert_eq!(
        fixture
            .store
            .load_explore(&"review".into(), &request.instance)
            .unwrap()
            .unwrap(),
        fixture.pass
    );
}

#[test]
fn one_view_keeps_latest_edits_without_rewriting_domain() {
    let fixture = Investigation::new();
    let path = fixture
        .store
        .explore_path(&"review".into(), &fixture.pass.exploration.instance)
        .unwrap();
    let before = std::fs::read(&path).unwrap();
    for (sequence, text) in [(1, "First edits"), (2, "Latest edits")] {
        fixture
            .store
            .save_explore_view(&"review".into(), &fixture.view(sequence, text))
            .unwrap();
    }
    fixture
        .store
        .save_explore_view(&"review".into(), &fixture.view(1, "stale"))
        .unwrap();
    let view = fixture
        .store
        .load_explore_view(&"review".into(), &fixture.pass.exploration.instance)
        .unwrap()
        .unwrap();
    assert_eq!(view.sequence, 2);
    assert_eq!(view.state.tasks["conclusion"].text, "Latest edits");
    assert!(!path.with_extension("views").exists());
    assert_eq!(std::fs::read(path).unwrap(), before);
}

#[test]
fn new_pass_keeps_previous_records_and_blocks_stale_domain_mutations() {
    let fixture = Investigation::new();
    let old = fixture.pass.exploration.instance.clone();
    let new = ExplorePass::new(Exploration::new(
        fixture.pass.exploration.comparison.clone(),
    ));
    fixture.store.create_explore(new.clone()).unwrap();
    assert_eq!(
        fixture
            .store
            .load_explore_history(&"review".into())
            .unwrap()
            .passes,
        vec![old.clone(), new.exploration.instance]
    );
    assert!(
        fixture
            .store
            .update_explore(&"review".into(), &old, |_| Ok(()))
            .is_err()
    );
    assert_eq!(
        fixture
            .store
            .load_explore(&"review".into(), &old)
            .unwrap()
            .unwrap(),
        fixture.pass
    );
}

#[test]
fn corrupt_unsupported_and_oversized_records_are_errors_and_remain_untouched() {
    let fixture = Investigation::new();
    let path = fixture
        .store
        .explore_path(&"review".into(), &fixture.pass.exploration.instance)
        .unwrap();
    for bytes in [
        b"broken".to_vec(),
        serde_json::to_vec(&Stored {
            version: 999,
            value: &fixture.pass,
        })
        .unwrap(),
    ] {
        std::fs::write(&path, &bytes).unwrap();
        assert!(
            fixture
                .store
                .load_explore(&"review".into(), &fixture.pass.exploration.instance)
                .is_err()
        );
        assert!(
            fixture
                .store
                .update_explore(
                    &"review".into(),
                    &fixture.pass.exploration.instance,
                    |_| Ok(())
                )
                .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(MAX_DOMAIN + 1).unwrap();
    assert!(
        fixture
            .store
            .load_explore(&"review".into(), &fixture.pass.exploration.instance)
            .is_err()
    );
    assert_eq!(file.metadata().unwrap().len(), MAX_DOMAIN + 1);
}

#[test]
fn a_long_pass_is_not_limited_to_one_mcp_response() {
    let mut fixture = Investigation::new();
    for number in 0..24 {
        let request = fixture.request(None, None);
        let mut update = Investigation::update(&request, number);
        update.reply.as_mut().unwrap().text = "context ".repeat(8_000);
        fixture.submit(update);
    }
    let path = fixture
        .store
        .explore_path(&"review".into(), &fixture.pass.exploration.instance)
        .unwrap();
    assert!(std::fs::metadata(path).unwrap().len() > 1024 * 1024);
    assert_eq!(
        fixture
            .store
            .load_explore(&"review".into(), &fixture.pass.exploration.instance)
            .unwrap()
            .unwrap()
            .exploration
            .questions
            .len(),
        24
    );
}

#[test]
fn broken_agenda_references_are_storage_errors_and_preserve_the_record() {
    let mut fixture = Investigation::new();
    let request = fixture.request(None, None);
    fixture.submit(Investigation::update(&request, 1));
    let path = fixture
        .store
        .explore_path(&"review".into(), &request.instance)
        .unwrap();
    for damage in 0..3 {
        let mut pass = fixture.pass.clone();
        match damage {
            0 => pass
                .exploration
                .topics
                .get_mut("topic1")
                .unwrap()
                .prerequisites
                .push("missing".into()),
            1 | 2 => {
                pass.exploration.conversation[0]
                    .update
                    .agenda
                    .push(review_explore::AgendaChange {
                        topic: if damage == 1 { "missing" } else { "topic1" }.into(),
                        action: review_explore::AgendaAction::Supersede,
                        reason: "Recorded reason".into(),
                        answer: None,
                        evidence: vec![],
                        replacement: Some("missing".into()),
                        decision: None,
                    });
            }
            _ => unreachable!(),
        }
        let bytes = serde_json::to_vec(&Stored {
            version: VERSION,
            value: pass,
        })
        .unwrap();
        std::fs::write(&path, &bytes).unwrap();
        assert!(
            fixture
                .store
                .load_explore(&"review".into(), &request.instance)
                .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}
