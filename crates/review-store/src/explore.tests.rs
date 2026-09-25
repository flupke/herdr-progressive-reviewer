use super::*;
use crate::LoadResult;
use review_explore::{
    AnswerInput, Comparison, Conclusion, Exploration, ExplorePass, InterviewUpdate, Question,
    Topic, TopicStatus,
};
use std::sync::Arc;

#[path = "explore_delivery.tests.rs"]
mod delivery;

#[test]
fn conclusion_with_partial_answer_coverage_marks_exact_baseline_once() {
    assert_explicit_conclusion(1, 50);
}

#[test]
fn full_answer_coverage_still_allows_questions_before_explicit_conclusion() {
    assert_explicit_conclusion(2, 100);
}

#[allow(
    clippy::too_many_lines,
    reason = "One end-to-end local finalization transaction"
)]
fn assert_explicit_conclusion(cited_lines: u32, expected_percent: u8) {
    use review_repository::repository::{
        ChangeKind, ChangedFile, DiffStatistics, FileKind, RepoPath, SnapshotId, SnapshotIdentity,
    };
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("policy.rs"), "first\nsecond\n").unwrap();
    let store = ReviewStore::open(directory.path().join("state"), directory.path()).unwrap();
    let comparison = Arc::new(Comparison {
        repository_root: directory.path().to_owned(),
        checkpoint: review_guide::ReviewCheckpoint::new("aabb", "ccdd"),
        files: vec![ChangedFile {
            old_path: None,
            new_path: Some(RepoPath::from_bytes(b"policy.rs".as_slice())),
            old_kind: FileKind::Absent,
            new_kind: FileKind::File,
            change: ChangeKind::Added,
            display_path: "policy.rs".into(),
            statistics: DiffStatistics {
                lines_added: 2,
                lines_removed: 0,
            },
        }],
        context: vec![],
        diffs: vec![
            b"diff --git a/policy.rs b/policy.rs\n@@ -0,0 +1,2 @@\n+first\n+second\n".to_vec(),
        ],
        manifest: vec![],
        sources: vec![],
        base: Some(SnapshotIdentity::Git {
            base_tree: "aabb".into(),
            display_id: "base".into(),
            snapshot_id: SnapshotId::from("ccdd".to_owned()),
        }),
    });
    let mut exploration = Exploration::new(comparison);
    let kickoff = exploration.request(None, None).unwrap();
    let mut pass = ExplorePass::new(Exploration::new(exploration.comparison.clone()));
    pass.exploration.instance = kickoff.instance.clone();
    pass.post(&kickoff).unwrap();
    store.create_explore(pass).unwrap();
    let question = |id: &str,
                    request: &review_explore::TurnRequest,
                    line: u32|
     -> InterviewUpdate {
        serde_json::from_value(serde_json::json!({
            "instance": request.instance, "request": request.request, "checkpoint": request.checkpoint,
            "interpretation": null, "reply": null, "agenda": [],
            "topics": [{"id":id,"title":"Policy","entries":[],"status":"open"}],
            "next": {"id":id,"version":1,"topic":id,"text":"Explain this line?",
                "alternatives":[{"id":"keep","text":"Keep it","outcome":"accepted"},{"id":"change","text":"Change it","outcome":"needs_follow_up"}],
                "evidence":[{"path":"policy.rs","side":"new","lines":{"first_line":line,"last_line":line},"notes":"Implements policy and changes the outcome"}],
                "supporting":[]}, "conclusion":null,"limitations":[],"findings":[]
        })).unwrap()
    };
    let mut first = question("q1", &kickoff, 1);
    first.next.as_mut().unwrap().evidence[0]
        .location
        .lines
        .as_mut()
        .unwrap()
        .last_line = cited_lines;
    let (applied, pass, feedback) = store
        .submit_explore(&"aabb".into(), &kickoff.instance, &first, false)
        .unwrap();
    assert!(applied);
    assert!(matches!(
        feedback,
        review_explore::CoverageReceipt::AfterAnswer { .. }
    ));
    assert_eq!(feedback.feedback().summary.percent, Some(expected_percent));
    assert!(feedback.feedback().awaiting_answer.is_empty());
    assert_eq!(pass.coverage.summary(false).percent, Some(0));
    assert!(pass.coverage.answers.is_empty());
    let first_receipt = feedback.clone();
    let shown = pass.exploration.questions.last().unwrap().clone();
    let answer = pass
        .exploration
        .clone()
        .request(
            Some(AnswerInput {
                text: "Discussed".into(),
                ..Default::default()
            }),
            Some(&shown),
        )
        .unwrap();
    store
        .update_explore(&"aabb".into(), &kickoff.instance, |pass| {
            pass.post(&answer).map_err(|error| error.to_string())
        })
        .unwrap();
    let (applied, _, replayed) = store
        .submit_explore(&"aabb".into(), &kickoff.instance, &first, false)
        .unwrap();
    assert!(!applied);
    assert_eq!(
        replayed, first_receipt,
        "retries retain the original coverage revision"
    );
    let pass = store
        .load_explore(&"aabb".into(), &kickoff.instance)
        .unwrap()
        .unwrap();
    assert!(
        pass.completion.is_none(),
        "coverage never creates a conclusion"
    );
    assert_eq!(pass.coverage.summary(false).percent, Some(expected_percent));
    assert_eq!(
        store.load(&"aabb".into(), b"policy.rs").unwrap(),
        LoadResult::Unreviewed,
        "answered evidence alone never marks files"
    );
    let conclusion = InterviewUpdate {
        instance: answer.instance.clone(),
        request: answer.request.clone(),
        checkpoint: answer.checkpoint.clone(),
        interpretation: None,
        reply: Some(review_explore::Reply {
            text: "Acknowledged".into(),
            evidence: vec![],
        }),
        agenda: vec![],
        topics: vec![],
        next: None,
        conclusion: Some(Conclusion {
            summary: "Concepts explored; remaining source inspected with no further question."
                .into(),
            to_be_implemented: String::new(),
            future_work: String::new(),
        }),
        limitations: vec![],
        findings: vec![],
    };
    let mut incomplete = pass.clone();
    incomplete.coverage.inventory.complete = false;
    incomplete.coverage.inventory.limitations = vec!["Missing comparison geometry".into()];
    let pending = incomplete.clone();
    let rejected = incomplete
        .submit(&conclusion, false)
        .unwrap_err()
        .to_string();
    assert!(rejected.contains("coverage_incomplete"), "{rejected}");
    assert_eq!(
        incomplete, pending,
        "inventory failure retains the pending turn"
    );
    let mut followup = question("q2", &answer, 1);
    followup.reply = Some(review_explore::Reply {
        text: "Acknowledged".into(),
        evidence: vec![],
    });
    let (_, pass, feedback) = store
        .submit_explore(&"aabb".into(), &kickoff.instance, &followup, false)
        .unwrap();
    assert_eq!(feedback.feedback().summary.percent, Some(expected_percent));
    assert!(
        pass.completion.is_none(),
        "another concept can be explored at 100%"
    );
    let shown = pass.exploration.questions.last().unwrap().clone();
    let answer2 = pass
        .exploration
        .clone()
        .request(
            Some(AnswerInput {
                text: "Also discussed".into(),
                ..Default::default()
            }),
            Some(&shown),
        )
        .unwrap();
    store
        .update_explore(&"aabb".into(), &kickoff.instance, |pass| {
            pass.post(&answer2).map_err(|error| error.to_string())
        })
        .unwrap();
    let mut conclusion = conclusion;
    conclusion.request = answer2.request.clone();
    conclusion.reply = Some(review_explore::Reply {
        text: "Acknowledged".into(),
        evidence: vec![],
    });
    let (applied, pass, feedback) = store
        .submit_explore(&"aabb".into(), &kickoff.instance, &conclusion, false)
        .unwrap();
    assert!(applied);
    assert!(matches!(
        feedback,
        review_explore::CoverageReceipt::Current(_)
    ));
    assert_eq!(feedback.feedback().summary.percent, Some(expected_percent));
    assert_eq!(
        feedback.feedback().summary.remaining,
        u64::from(2 - cited_lines)
    );
    assert!(pass.completion.as_ref().unwrap().completed);
    assert_eq!(
        pass.completion.as_ref().unwrap().summary,
        feedback.feedback().summary
    );
    assert_eq!(
        pass.completion
            .as_ref()
            .unwrap()
            .unexplored
            .as_ref()
            .unwrap()
            .required,
        pass.coverage.remaining(false),
        "completion freezes unanswered changes before marking the files"
    );
    let restored = store
        .load_explore(&"aabb".into(), &kickoff.instance)
        .unwrap()
        .unwrap();
    assert_eq!(restored, pass, "the coverage receipt survives reopening");
    let LoadResult::Reviewed(record) = store.load(&"aabb".into(), b"policy.rs").unwrap() else {
        panic!("no mark")
    };
    assert_eq!(record.baseline_commit_id, "ccdd");
    store.unreview(&"aabb".into(), b"policy.rs").unwrap();
    let (applied, _, _) = store
        .submit_explore(&"aabb".into(), &kickoff.instance, &conclusion, false)
        .unwrap();
    assert!(!applied);
    assert_eq!(
        store.load(&"aabb".into(), b"policy.rs").unwrap(),
        LoadResult::Unreviewed,
        "accepted retry must not recreate a manually removed mark"
    );
    let followup = pass.exploration.clone().request(None, None).unwrap();
    store
        .update_explore(&"aabb".into(), &kickoff.instance, |pass| {
            pass.post(&followup).map_err(|error| error.to_string())
        })
        .unwrap();
    let later = question("q3", &followup, 1);
    let (applied, _, _) = store
        .submit_explore(&"aabb".into(), &kickoff.instance, &later, false)
        .unwrap();
    assert!(applied, "the conversation remains open after marking");
    assert!(
        store
            .load_explore(&"aabb".into(), &kickoff.instance)
            .unwrap()
            .is_some(),
        "a later response must leave a readable pass"
    );
    assert_eq!(
        store.load(&"aabb".into(), b"policy.rs").unwrap(),
        LoadResult::Unreviewed,
        "later conversation must not recreate completed marks"
    );
}

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
                "evidence":[{"path":"policy.rs","side":"new","lines":{"first_line":1,"last_line":1}, "notes":"Defines policy and determines retention"}]},
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
fn clearing_unreadable_explore_state_preserves_review_marks() {
    let fixture = Investigation::new();
    let unit = "review".into();
    let instance = &fixture.pass.exploration.instance;
    let pass_path = fixture.store.explore_path(&unit, instance).unwrap();
    let view_path = fixture.store.explore_view_path(&unit, instance).unwrap();
    fixture
        .store
        .save_explore_view(&unit, &fixture.view(1, "draft"))
        .unwrap();
    fixture
        .store
        .mark(&unit, b"policy.rs", &"a".repeat(40))
        .unwrap();
    std::fs::write(&pass_path, b"invalid saved pass").unwrap();

    fixture.store.clear_explore(&unit).unwrap();

    assert!(
        fixture
            .store
            .load_explore_history(&unit)
            .unwrap()
            .passes
            .is_empty()
    );
    assert!(!pass_path.exists());
    assert!(!view_path.exists());
    assert!(matches!(
        fixture.store.load(&unit, b"policy.rs").unwrap(),
        LoadResult::Reviewed(_)
    ));
    fixture.store.clear_explore(&unit).unwrap();
}

#[test]
fn clearing_unreadable_editor_view_keeps_the_interview() {
    let fixture = Investigation::new();
    let unit = "review".into();
    let instance = &fixture.pass.exploration.instance;
    let view_path = fixture.store.explore_view_path(&unit, instance).unwrap();
    std::fs::write(&view_path, b"invalid editor view").unwrap();

    fixture.store.clear_explore_view(&unit, instance).unwrap();

    assert!(!view_path.exists());
    assert!(
        fixture
            .store
            .load_explore(&unit, instance)
            .unwrap()
            .is_some()
    );
}

#[test]
fn clearing_one_unreadable_pass_preserves_other_saved_passes() {
    let fixture = Investigation::new();
    let unit = "review".into();
    let bad = fixture.pass.exploration.instance.clone();
    let good = ExplorePass::new(Exploration::new(
        fixture.pass.exploration.comparison.clone(),
    ));
    let good_instance = good.exploration.instance.clone();
    fixture.store.create_explore(good).unwrap();
    let good_view = review_explore::ViewSave {
        instance: good_instance.clone(),
        ..fixture.view(1, "retained editor")
    };
    fixture.store.save_explore_view(&unit, &good_view).unwrap();
    let bad_path = fixture.store.explore_path(&unit, &bad).unwrap();
    std::fs::write(&bad_path, b"invalid saved pass").unwrap();

    fixture.store.clear_explore_pass(&unit, &bad).unwrap();

    assert!(!bad_path.exists());
    assert_eq!(
        fixture.store.load_explore_history(&unit).unwrap().passes,
        vec![good_instance.clone()]
    );
    assert!(
        fixture
            .store
            .load_explore(&unit, &good_instance)
            .unwrap()
            .is_some()
    );
    assert_eq!(
        fixture
            .store
            .load_explore_view(&unit, &good_instance)
            .unwrap(),
        Some(good_view)
    );
}

#[test]
fn repairing_unreadable_index_preserves_readable_passes_and_views() {
    let fixture = Investigation::new();
    let unit = "review".into();
    let instance = fixture.pass.exploration.instance.clone();
    fixture
        .store
        .save_explore_view(&unit, &fixture.view(1, "draft"))
        .unwrap();
    let index = fixture
        .store
        .explore_review(&unit)
        .unwrap()
        .join("index.json");
    std::fs::write(&index, b"invalid index").unwrap();

    let repaired = fixture.store.repair_explore_history(&unit).unwrap();

    assert_eq!(repaired.passes, vec![instance.clone()]);
    assert!(!repaired.latest_editable);
    assert_eq!(fixture.store.load_explore_history(&unit).unwrap(), repaired);
    assert!(
        fixture
            .store
            .load_explore(&unit, &instance)
            .unwrap()
            .is_some()
    );
    assert!(
        fixture
            .store
            .load_explore_view(&unit, &instance)
            .unwrap()
            .is_some()
    );
}

#[test]
fn repaired_history_never_guesses_an_editable_latest_pass() {
    let fixture = Investigation::new();
    let unit = "review".into();
    let second = ExplorePass::new(Exploration::new(
        fixture.pass.exploration.comparison.clone(),
    ));
    fixture.store.create_explore(second).unwrap();
    let index = fixture
        .store
        .explore_review(&unit)
        .unwrap()
        .join("index.json");
    std::fs::write(&index, b"invalid index").unwrap();

    let repaired = fixture.store.repair_explore_history(&unit).unwrap();
    assert_eq!(repaired.passes.len(), 2);
    assert!(!repaired.latest_editable);
    let guessed_latest = repaired.passes.last().unwrap();
    assert!(
        fixture
            .store
            .update_explore(&unit, guessed_latest, |_| Ok(()))
            .is_err()
    );

    let fresh = ExplorePass::new(Exploration::new(
        fixture.pass.exploration.comparison.clone(),
    ));
    let fresh_instance = fresh.exploration.instance.clone();
    fixture.store.create_explore(fresh).unwrap();
    let history = fixture.store.load_explore_history(&unit).unwrap();
    assert!(history.latest_editable);
    assert_eq!(history.passes.last(), Some(&fresh_instance));
    assert!(
        fixture
            .store
            .update_explore(&unit, &fresh_instance, |_| Ok(()))
            .is_ok()
    );
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
