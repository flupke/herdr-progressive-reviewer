//! The saved-pass rules over a temporary store.

use std::sync::Arc;

use review_explore::{
    AnswerInput, Comparison, Conclusion, ConversationBinding, DispatchId, DispatchResult,
    DispatchState, Exploration, ExplorePass, InterviewUpdate, Question, ViewSave,
};
use review_store::{LoadResult, ReviewStore};

use super::SavedPasses;

/// The directory of the only review saved in `store`.
fn review_directory(store: &ReviewStore) -> std::path::PathBuf {
    std::fs::read_dir(store.explore_directory())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path()
}

struct Investigation {
    _directory: tempfile::TempDir,
    store: ReviewStore,
    passes: SavedPasses,
    pass: ExplorePass,
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
        let pass = ExplorePass::new(Exploration::new(Arc::new(comparison)));
        let passes = SavedPasses::new(store.clone());
        passes.create(pass.clone()).unwrap();
        Self {
            _directory: directory,
            store,
            passes,
            pass,
        }
    }

    fn mutate<T>(
        &mut self,
        f: impl FnOnce(&mut ExplorePass) -> std::result::Result<T, String>,
    ) -> T {
        let (value, pass) = self
            .passes
            .update(&"review".into(), &self.pass.exploration.instance, f)
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

    fn path(&self, instance: &str) -> std::path::PathBuf {
        review_directory(&self.store).join(format!("{instance}.json"))
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
fn conclusion_with_partial_answer_coverage_leaves_files_unreviewed() {
    assert_explicit_conclusion(1, 50);
}

#[test]
fn full_answer_coverage_allows_questions_and_conclusion_preserves_existing_marks() {
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
    let passes = SavedPasses::new(store.clone());
    let comparison = Arc::new(Comparison {
        repository_root: directory.path().to_owned(),
        checkpoint: review_source::ReviewCheckpoint::new("aabb", "ccdd"),
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
    passes.create(pass).unwrap();
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
    let (applied, pass, feedback) = passes
        .submit(&"aabb".into(), &kickoff.instance, &first, false)
        .unwrap();
    assert!(applied);
    assert!(matches!(
        feedback,
        review_explore::CoverageReceipt::AfterAnswer { .. }
    ));
    assert_eq!(
        feedback.feedback().summary.answered_required_units_percent,
        Some(expected_percent)
    );
    assert!(feedback.feedback().awaiting_answer.is_empty());
    assert_eq!(
        pass.coverage.summary(false).answered_required_units_percent,
        Some(0)
    );
    assert_eq!(
        pass.coverage.changed_line_coverage(None).explored,
        0,
        "projected feedback credits nothing"
    );
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
    passes
        .update(&"aabb".into(), &kickoff.instance, |pass| {
            pass.post(&answer).map_err(|error| error.to_string())
        })
        .unwrap();
    let (applied, _, replayed) = passes
        .submit(&"aabb".into(), &kickoff.instance, &first, false)
        .unwrap();
    assert!(!applied);
    assert_eq!(
        replayed, first_receipt,
        "retries retain the original coverage revision"
    );
    let pass = passes
        .pass(&"aabb".into(), &kickoff.instance)
        .unwrap()
        .unwrap();
    assert!(
        pass.completion.is_none(),
        "coverage never creates a conclusion"
    );
    assert_eq!(
        pass.coverage.summary(false).answered_required_units_percent,
        Some(expected_percent)
    );
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
        inspections: vec![review_explore::Inspection {
            sources: vec![review_explore::CodeLocation {
                path: review_repository::repository::RepoPath::from_bytes(b"policy.rs"),
                side: review_explore::SourceSide::New,
                lines: Some(review_source::SourceLineRange {
                    first_line: 2,
                    last_line: 2,
                }),
            }],
            behavior: "Second policy line after the answer".into(),
            finding: "This line does not introduce another decision".into(),
            uncertainty: String::new(),
            disposition: review_explore::InspectionDisposition::NoFurtherInquiry {
                reason: "Its behavior is already established by the answered first line".into(),
            },
        }],
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
    let mut without_geometry = (*incomplete.exploration.comparison).clone();
    without_geometry.diffs.clear();
    incomplete.coverage = review_explore::CoverageLedger::new(&without_geometry);
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
    let (_, pass, feedback) = passes
        .submit(&"aabb".into(), &kickoff.instance, &followup, false)
        .unwrap();
    assert_eq!(
        feedback.feedback().summary.answered_required_units_percent,
        Some(expected_percent)
    );
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
    passes
        .update(&"aabb".into(), &kickoff.instance, |pass| {
            pass.post(&answer2).map_err(|error| error.to_string())
        })
        .unwrap();
    let mut conclusion = conclusion;
    conclusion.request = answer2.request.clone();
    conclusion.reply = Some(review_explore::Reply {
        text: "Acknowledged".into(),
        evidence: vec![],
    });
    if cited_lines == 1 {
        let mut without_inspection = conclusion.clone();
        without_inspection.inspections.clear();
        let before = passes
            .pass(&"aabb".into(), &kickoff.instance)
            .unwrap()
            .unwrap();
        let error = passes
            .submit(
                &"aabb".into(),
                &kickoff.instance,
                &without_inspection,
                false,
            )
            .unwrap_err()
            .to_string();
        assert!(error.contains("inspection_incomplete"), "{error}");
        assert_eq!(
            passes
                .pass(&"aabb".into(), &kickoff.instance)
                .unwrap()
                .unwrap(),
            before
        );
    }
    if cited_lines == 2 {
        store
            .mark(&"aabb".into(), b"policy.rs", "aabb0011")
            .unwrap();
    }
    let prior_mark = store.load(&"aabb".into(), b"policy.rs").unwrap();
    let (applied, pass, feedback) = passes
        .submit(&"aabb".into(), &kickoff.instance, &conclusion, false)
        .unwrap();
    assert!(applied);
    assert!(matches!(
        feedback,
        review_explore::CoverageReceipt::Current(_)
    ));
    assert_eq!(
        feedback.feedback().summary.answered_required_units_percent,
        Some(expected_percent)
    );
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
        "completion freezes unanswered changes independently of file review marks"
    );
    let restored = passes
        .pass(&"aabb".into(), &kickoff.instance)
        .unwrap()
        .unwrap();
    assert_eq!(restored, pass, "the coverage receipt survives reopening");
    assert_eq!(
        store.load(&"aabb".into(), b"policy.rs").unwrap(),
        prior_mark
    );
    assert_legacy_completion_preserves_marks(&store, &pass, &prior_mark);
    store.unreview(&"aabb".into(), b"policy.rs").unwrap();
    let (applied, _, _) = passes
        .submit(&"aabb".into(), &kickoff.instance, &conclusion, false)
        .unwrap();
    assert!(!applied);
    assert_eq!(
        store.load(&"aabb".into(), b"policy.rs").unwrap(),
        LoadResult::Unreviewed,
        "accepted retry must not recreate a manually removed mark"
    );
    let followup = pass.exploration.clone().request(None, None).unwrap();
    passes
        .update(&"aabb".into(), &kickoff.instance, |pass| {
            pass.post(&followup).map_err(|error| error.to_string())
        })
        .unwrap();
    let later = question("q3", &followup, 1);
    let (applied, _, _) = passes
        .submit(&"aabb".into(), &kickoff.instance, &later, false)
        .unwrap();
    assert!(applied, "the conversation remains open after concluding");
    assert!(
        passes
            .pass(&"aabb".into(), &kickoff.instance)
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

fn assert_legacy_completion_preserves_marks(
    store: &ReviewStore,
    pass: &ExplorePass,
    prior: &LoadResult,
) {
    let unit = &pass.exploration.comparison.checkpoint.review_unit;
    let instance = &pass.exploration.instance;
    let mut legacy = serde_json::to_value(pass).unwrap();
    legacy["completion"]["completed"] = false.into();
    legacy["completion"]["marks"] = serde_json::json!([{
        "path": b"policy.rs", "prior": null, "applied": false
    }]);
    std::fs::write(
        review_directory(store).join(format!("{instance}.json")),
        serde_json::to_vec(&serde_json::json!({"version": 1, "value": legacy})).unwrap(),
    )
    .unwrap();
    let recovered = SavedPasses::new(store.clone())
        .recover_completion(unit, instance)
        .unwrap();
    assert!(recovered.completion.unwrap().completed);
    assert_eq!(&store.load(unit, b"policy.rs").unwrap(), prior);
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
    fixture.passes.create(good).unwrap();
    let good_view = review_explore::ViewSave {
        instance: good_instance.clone(),
        ..fixture.view(1, "retained editor")
    };
    fixture.passes.save_view(&unit, &good_view).unwrap();
    let bad_path = fixture.path(&bad);
    std::fs::write(&bad_path, b"invalid saved pass").unwrap();

    fixture.passes.clear_pass(&unit, &bad).unwrap();

    assert!(!bad_path.exists());
    assert_eq!(
        fixture.passes.history(&unit).unwrap().passes,
        vec![good_instance.clone()]
    );
    assert!(
        fixture
            .passes
            .pass(&unit, &good_instance)
            .unwrap()
            .is_some()
    );
    assert_eq!(
        fixture.passes.view(&unit, &good_instance).unwrap(),
        Some(good_view)
    );
}

#[test]
fn repairing_unreadable_index_preserves_readable_passes_and_views() {
    let fixture = Investigation::new();
    let unit = "review".into();
    let instance = fixture.pass.exploration.instance.clone();
    fixture
        .passes
        .save_view(&unit, &fixture.view(1, "draft"))
        .unwrap();
    let index = review_directory(&fixture.store).join("index.json");
    std::fs::write(&index, b"invalid index").unwrap();

    let repaired = fixture.passes.repair_history(&unit).unwrap();

    assert_eq!(repaired.passes, vec![instance.clone()]);
    assert!(!repaired.latest_editable);
    assert_eq!(fixture.passes.history(&unit).unwrap(), repaired);
    assert!(fixture.passes.pass(&unit, &instance).unwrap().is_some());
    assert!(fixture.passes.view(&unit, &instance).unwrap().is_some());
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
    let result = fixture.passes.update(
        &"review".into(),
        &fixture.pass.exploration.instance,
        |pass| pass.exploration.submit(conflict).map_err(|e| e.to_string()),
    );
    assert!(result.is_err());
    assert_eq!(
        fixture
            .passes
            .pass(&"review".into(), &request.instance)
            .unwrap()
            .unwrap(),
        fixture.pass
    );
}

#[test]
fn one_view_keeps_latest_edits_without_rewriting_domain() {
    let fixture = Investigation::new();
    let path = fixture.path(&fixture.pass.exploration.instance);
    let before = std::fs::read(&path).unwrap();
    for (sequence, text) in [(1, "First edits"), (2, "Latest edits")] {
        fixture
            .passes
            .save_view(&"review".into(), &fixture.view(sequence, text))
            .unwrap();
    }
    fixture
        .passes
        .save_view(&"review".into(), &fixture.view(1, "stale"))
        .unwrap();
    let view = fixture
        .passes
        .view(&"review".into(), &fixture.pass.exploration.instance)
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
    fixture.passes.create(new.clone()).unwrap();
    assert_eq!(
        fixture.passes.history(&"review".into()).unwrap().passes,
        vec![old.clone(), new.exploration.instance]
    );
    assert!(
        fixture
            .passes
            .update(&"review".into(), &old, |_| Ok(()))
            .is_err()
    );
    assert_eq!(
        fixture
            .passes
            .pass(&"review".into(), &old)
            .unwrap()
            .unwrap(),
        fixture.pass
    );
}

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
            completed: true,
            exclusions_enabled: false,
            summary: review_explore::CoverageSummary::default(),
            unexplored: None,
        });
        Ok(())
    });
    let request = fixture
        .pass
        .exploration
        .implementation("Only the edited task".into())
        .unwrap();
    fixture.mutate(|pass| {
        pass.last_agent_session = ConversationBinding::from_agent(&agent());
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
        .passes
        .finish_dispatch(
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
        .passes
        .create(ExplorePass::new(Exploration::new(
            fixture.pass.exploration.comparison.clone(),
        )))
        .unwrap();
    for state in [DispatchState::Delivered, DispatchState::Cancelled] {
        let pass = fixture
            .passes
            .finish_dispatch(
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
        .passes
        .finish_dispatch(
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
            .passes
            .update(&"review".into(), &request.instance, |pass| pass
                .authorize(&changed)
                .map_err(|e| e.to_string()))
            .is_err()
    );
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
            .passes
            .update(&"review".into(), &old.instance, |pass| pass
                .begin_dispatch(&id, &agent())
                .map_err(|e| e.to_string()))
            .is_err()
    );
    let new = fixture.request(None, None);
    assert!(
        fixture
            .passes
            .update(&"review".into(), &old.instance, |pass| pass
                .post(&old)
                .map_err(|e| e.to_string()))
            .is_err()
    );
    let saved = fixture
        .passes
        .pass(&"review".into(), &old.instance)
        .unwrap()
        .unwrap();
    assert_eq!(saved.exploration.pending_request(), Some(&new));
    assert_eq!(saved.exploration.retry_request(), Some(&new));
}

#[test]
fn editor_sequence_conflict_and_wrong_review_keep_original_bytes() {
    let fixture = Investigation::new();
    let view = fixture.view(1, "Original text");
    fixture.passes.save_view(&"review".into(), &view).unwrap();
    assert!(
        fixture
            .passes
            .save_view(&"review".into(), &fixture.view(1, "Conflicting text"))
            .is_err()
    );
    assert!(
        fixture
            .passes
            .save_view(&"different-review".into(), &view)
            .is_err()
    );
    assert_eq!(
        fixture
            .passes
            .view(&"review".into(), &view.instance)
            .unwrap()
            .unwrap(),
        view
    );
}
