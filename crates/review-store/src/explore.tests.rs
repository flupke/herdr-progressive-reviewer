use super::*;
use review_explore::{
    AnswerInput, Comparison, Conclusion, Exploration, ExploreRound, InterviewUpdate, Question,
    Topic, TopicStatus,
};
use std::sync::Arc;

#[path = "explore_format.tests.rs"]
mod format;

struct Investigation {
    directory: tempfile::TempDir,
    store: ReviewStore,
    round: ExploreRound,
}

impl Investigation {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("policy.rs"), "fn policy() {}\n").unwrap();
        let store = ReviewStore::open(directory.path().join("state"), directory.path()).unwrap();
        let comparison = Comparison {
            repository_root: directory.path().to_owned(),
            checkpoint: review_source::ReviewCheckpoint::new("review", "checkpoint"),
            files: vec![],
            context: vec![],
            diffs: vec![],
            manifest: vec![],
            sources: vec![],
            base: None,
        };
        let round = ExploreRound::new(Exploration::new(Arc::new(comparison)));
        let records = store.lock_explore(&"review".into()).unwrap();
        records.create_round(&round).unwrap();
        assert!(records.create_round(&round).is_err());
        records
            .save_history(&ExploreHistory {
                rounds: vec![round.exploration.instance.clone()],
                ..ExploreHistory::default()
            })
            .unwrap();
        drop(records);
        Self {
            directory,
            store,
            round,
        }
    }

    fn mutate<T>(
        &mut self,
        f: impl FnOnce(&mut ExploreRound) -> std::result::Result<T, String>,
    ) -> T {
        let (value, round) = self
            .store
            .lock_explore(&"review".into())
            .unwrap()
            .update_round(&self.round.exploration.instance, f)
            .unwrap();
        self.round = round;
        value
    }

    fn request(
        &mut self,
        input: Option<AnswerInput>,
        question: Option<&Question>,
    ) -> review_explore::TurnRequest {
        let request = self
            .round
            .exploration
            .clone()
            .request(input, question)
            .unwrap();
        self.mutate(|round| round.post(&request).map_err(|e| e.to_string()));
        request
    }

    fn submit(&mut self, update: InterviewUpdate) -> bool {
        self.mutate(|round| round.exploration.submit(update).map_err(|e| e.to_string()))
    }

    fn update(request: &review_explore::TurnRequest, number: usize) -> InterviewUpdate {
        serde_json::from_value(serde_json::json!({
            "instance":request.instance, "request":request.request, "checkpoint": request.checkpoint,
            "interpretation":null, "reply":{"text":"Opening or direct answer", "evidence":[]},
            "topics":[{"id":format!("topic{number}"),"title":"Policy", "entries":[],"status":"open"}],
            "next":{"id":format!("q{number}"), "version":1, "topic":format!("topic{number}"),"text":"Which policy?",
                "alternatives":[{"id":"keep","text":"Keep the complete policy", "outcome":"accepted"},{"id":"change","text":"Change policy", "outcome":"needs_follow_up"}],
                "evidence":[{"path":"policy.rs","side":"new","lines":{"first_line":1,"last_line":1}, "notes":"Defines policy and determines retention"}]},
            "design": request.is_kickoff().then(|| serde_json::json!({
                "thesis":"A policy.", "overview":{"thesis":"A policy.", "body":"A policy."}, "data_flow":{"thesis":"None.", "body":"None."}, "algorithm":{"thesis":"None.", "body":"None."}, "alternatives":{"thesis":"None.", "body":"None."}
            })),
            "conclusion":null, "limitations":[], "findings":[]
        })).unwrap()
    }

    fn view(&self, sequence: u64, text: &str) -> ViewSave {
        ViewSave {
            review_unit: self
                .round
                .exploration
                .comparison
                .checkpoint
                .review_unit
                .clone(),
            instance: self.round.exploration.instance.clone(),
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
    let question = fixture.round.exploration.questions[0].clone();
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
            ..Default::default()
        }),
        Some(&question),
    );
    let mut update = Investigation::update(&correction, 3);
    update.agenda = vec![serde_json::from_value(serde_json::json!({"topic":"unused","action":"retire","reason":"Reviewer corrected the premise","answer":correction.answer.as_ref().unwrap().id,"evidence":[],"replacement":null,"decision":null})).unwrap()];
    fixture.submit(update);
    for number in 4..=5 {
        let question = fixture.round.exploration.questions.last().cloned();
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
            quiz: Vec::new(),
            quiz_empty_reason: Some("Nothing at whiteboard level.".into()),
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
    let expected = fixture.round.clone();
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
fn corrupt_unsupported_and_oversized_records_are_errors_and_remain_untouched() {
    let fixture = Investigation::new();
    let path = fixture
        .store
        .explore_path(&"review".into(), &fixture.round.exploration.instance)
        .unwrap();
    for bytes in [
        b"broken".to_vec(),
        serde_json::to_vec(&Stored {
            version: 999,
            value: &fixture.round,
        })
        .unwrap(),
    ] {
        std::fs::write(&path, &bytes).unwrap();
        assert!(
            fixture
                .store
                .load_explore(&"review".into(), &fixture.round.exploration.instance)
                .is_err()
        );
        assert!(
            fixture
                .store
                .lock_explore(&"review".into())
                .unwrap()
                .update_round(&fixture.round.exploration.instance, |_| Ok(()))
                .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(MAX_DOMAIN + 1).unwrap();
    assert!(
        fixture
            .store
            .load_explore(&"review".into(), &fixture.round.exploration.instance)
            .is_err()
    );
    assert_eq!(file.metadata().unwrap().len(), MAX_DOMAIN + 1);
}

#[test]
fn a_long_round_is_not_limited_to_one_mcp_response() {
    let mut fixture = Investigation::new();
    for number in 0..24 {
        let request = fixture.request(None, None);
        let mut update = Investigation::update(&request, number);
        update.reply.as_mut().unwrap().text = "context ".repeat(8_000);
        fixture.submit(update);
    }
    let path = fixture
        .store
        .explore_path(&"review".into(), &fixture.round.exploration.instance)
        .unwrap();
    assert!(std::fs::metadata(path).unwrap().len() > 1024 * 1024);
    assert_eq!(
        fixture
            .store
            .load_explore(&"review".into(), &fixture.round.exploration.instance)
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
        let mut round = fixture.round.clone();
        match damage {
            0 => round
                .exploration
                .topics
                .get_mut("topic1")
                .unwrap()
                .prerequisites
                .push("missing".into()),
            1 | 2 => {
                round.exploration.conversation[0].update.agenda.push(
                    review_explore::AgendaChange {
                        topic: if damage == 1 { "missing" } else { "topic1" }.into(),
                        action: review_explore::AgendaAction::Supersede,
                        reason: "Recorded reason".into(),
                        answer: None,
                        evidence: vec![],
                        replacement: Some("missing".into()),
                        decision: None,
                    },
                );
            }
            _ => unreachable!(),
        }
        let bytes = serde_json::to_vec(&Stored {
            version: VERSION,
            value: round,
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

#[test]
fn simultaneous_updates_see_the_latest_revision_instead_of_overwriting_each_other() {
    let fixture = Investigation::new();
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let workers: Vec<_> = (0..2)
        .map(|_| {
            let request = fixture
                .round
                .exploration
                .clone()
                .request(None, None)
                .unwrap();
            let store = fixture.store.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store
                    .lock_explore(&"review".into())
                    .unwrap()
                    .update_round(&request.instance, |round| {
                        round.post(&request).map_err(|e| e.to_string())
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
    let round = fixture
        .store
        .load_explore(&"review".into(), &fixture.round.exploration.instance)
        .unwrap()
        .unwrap();
    assert_eq!(round.turns.len(), 1);
    assert!(round.exploration.pending_request().is_some());
}

#[test]
fn an_unchanged_update_writes_nothing_and_a_change_takes_the_next_revision() {
    let mut fixture = Investigation::new();
    let path = fixture
        .store
        .explore_path(&"review".into(), &fixture.round.exploration.instance)
        .unwrap();
    let before = std::fs::read(&path).unwrap();
    fixture.mutate(|_| Ok(()));
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(fixture.round.revision, 0);

    fixture.request(None, None);
    assert_eq!(fixture.round.revision, 1);
    assert_ne!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn a_saved_view_leaves_the_round_untouched_and_belongs_to_its_review() {
    let fixture = Investigation::new();
    let path = fixture
        .store
        .explore_path(&"review".into(), &fixture.round.exploration.instance)
        .unwrap();
    let before = std::fs::read(&path).unwrap();
    let records = fixture.store.lock_explore(&"review".into()).unwrap();
    let view = fixture.view(1, "Latest edits");
    records.save_view(&view).unwrap();
    let foreign = ViewSave {
        review_unit: "different-review".into(),
        ..fixture.view(2, "Another review")
    };
    assert!(records.save_view(&foreign).is_err());
    drop(records);

    assert_eq!(
        fixture
            .store
            .load_explore_view(&"review".into(), &view.instance)
            .unwrap(),
        Some(view)
    );
    assert!(!path.with_extension("views").exists());
    assert_eq!(std::fs::read(path).unwrap(), before);
}

#[test]
fn removing_one_round_and_its_view_keeps_other_records_and_lists_unreadable_files() {
    let fixture = Investigation::new();
    let unit: ReviewUnit = "review".into();
    let removed = fixture.round.exploration.instance.clone();
    let kept = ExploreRound::new(Exploration::new(
        fixture.round.exploration.comparison.clone(),
    ));
    let kept_view = ViewSave {
        instance: kept.exploration.instance.clone(),
        ..fixture.view(1, "retained editor")
    };
    let records = fixture.store.lock_explore(&unit).unwrap();
    records.create_round(&kept).unwrap();
    records.save_view(&kept_view).unwrap();
    records
        .save_view(&fixture.view(1, "removed editor"))
        .unwrap();
    std::fs::write(
        fixture.store.explore_path(&unit, &removed).unwrap(),
        b"invalid saved round",
    )
    .unwrap();
    let mut listed: Vec<_> = records
        .round_files()
        .unwrap()
        .into_iter()
        .map(|(instance, _)| instance)
        .collect();
    listed.sort();
    let mut expected = vec![removed.clone(), kept.exploration.instance.clone()];
    expected.sort();
    assert_eq!(listed, expected);

    records.remove_round(&removed).unwrap();
    records.remove_view(&removed).unwrap();
    records.remove_view(&removed).unwrap();
    records.sync().unwrap();

    assert!(records.round(&removed).unwrap().is_none());
    assert!(records.view(&removed).unwrap().is_none());
    assert_eq!(
        records.round(&kept.exploration.instance).unwrap(),
        Some(kept)
    );
    assert_eq!(records.view(&kept_view.instance).unwrap(), Some(kept_view));
}

#[test]
fn the_forks_of_a_round_are_saved_beside_it_even_once_it_is_closed_and_are_no_round() {
    let fixture = Investigation::new();
    let unit: ReviewUnit = "review".into();
    let instance = fixture.round.exploration.instance.clone();
    let records = fixture.store.lock_explore(&unit).unwrap();
    assert_eq!(
        records.round_forks(&instance).unwrap(),
        RoundForks::default()
    );
    let mut history = records.history().unwrap();
    history.closed = true;
    records.save_history(&history).unwrap();
    let fork = review_run_ahead::ForkRecord {
        question: "cache-eviction".into(),
        version: 1,
        choice: "keep".into(),
        session: "fork-session".into(),
        transcripts: "/state/projects".into(),
        reviewer: agent_fork::ProcessStamp { pid: 1, started: 2 },
        process: None,
        taken_at_ms: 3,
        turn: None,
        exit: None,
        usage: None,
        discarded: None,
        cleaned: false,
    };

    let ((), saved) = records
        .update_round_forks(&instance, |forks| forks.forks.push(fork.clone()))
        .unwrap();
    drop(records);

    assert_eq!(saved.forks, [fork]);
    assert_eq!(
        fixture.store.load_round_forks(&unit, &instance).unwrap(),
        saved
    );
    let listed: Vec<_> = fixture
        .store
        .lock_explore(&unit)
        .unwrap()
        .round_files()
        .unwrap()
        .into_iter()
        .map(|(instance, _)| instance)
        .collect();
    assert_eq!(listed, [instance]);
}
