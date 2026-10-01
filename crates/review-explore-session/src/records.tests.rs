//! The saved-pass rules over a temporary store.

use std::sync::Arc;

use review_explore::{
    AnswerInput, Comparison, Conclusion, ConversationBinding, DispatchId, DispatchResult,
    DispatchState, Exploration, ExplorePass, InterviewUpdate, Question, ViewSave,
};
use review_store::ReviewStore;

use super::{SavedPasses, Submitted};

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
fn a_conclusion_completes_the_pass_and_leaves_review_marks_alone() {
    let mut investigation = Investigation::new();
    let store = investigation.store.clone();
    let kickoff = investigation.request(None, None);
    assert!(investigation.submit(Investigation::update(&kickoff, 1)));
    let question = investigation.pass.exploration.questions[0].clone();
    let answer = investigation.request(
        Some(AnswerInput {
            text: "Discussed".into(),
            ..Default::default()
        }),
        Some(&question),
    );
    store
        .mark(
            &"review".into(),
            b"policy.rs",
            "aabb0011",
            &review_types::MarkAuthor::Reviewer,
        )
        .unwrap();
    let prior_mark = store.load(&"review".into(), b"policy.rs").unwrap();
    let mut conclusion = Investigation::update(&answer, 2);
    conclusion.next = None;
    conclusion.topics.clear();
    conclusion.conclusion = Some(Conclusion {
        summary: "Concepts explored.".into(),
        to_be_implemented: String::new(),
        future_work: String::new(),
    });
    let instance = investigation.pass.exploration.instance.clone();

    let Submitted { applied, pass } = investigation
        .passes
        .submit(&"review".into(), &instance, &conclusion)
        .unwrap();

    assert!(applied);
    assert!(pass.completion.is_some());
    assert_eq!(
        store.load(&"review".into(), b"policy.rs").unwrap(),
        prior_mark
    );
    let restored = investigation
        .passes
        .pass(&"review".into(), &instance)
        .unwrap()
        .unwrap();
    assert_eq!(restored, pass);
    let Submitted { applied, .. } = investigation
        .passes
        .submit(&"review".into(), &instance, &conclusion)
        .unwrap();
    assert!(!applied, "an identical retry changes nothing");
    let followup = pass.exploration.clone().request(None, None).unwrap();
    investigation
        .passes
        .update(&"review".into(), &instance, |pass| {
            pass.post(&followup).map_err(|error| error.to_string())
        })
        .unwrap();
    let Submitted { applied, .. } = investigation
        .passes
        .submit(
            &"review".into(),
            &instance,
            &Investigation::update(&followup, 3),
        )
        .unwrap();
    assert!(applied, "the conversation remains open after concluding");
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
