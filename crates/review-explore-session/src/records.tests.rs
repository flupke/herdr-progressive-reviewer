//! The saved-round rules over a temporary store.

use std::sync::Arc;

use review_explore::{
    AnswerInput, Comparison, Conclusion, ConversationBinding, DispatchId, DispatchResult,
    DispatchState, Exploration, ExploreRound, InterviewUpdate, Question, ViewSave,
};
use review_store::ReviewStore;

use super::{SavedRounds, Submitted};

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
    rounds: SavedRounds,
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
        let rounds = SavedRounds::new(store.clone());
        rounds.create(round.clone()).unwrap();
        Self {
            _directory: directory,
            store,
            rounds,
            round,
        }
    }

    fn mutate<T>(
        &mut self,
        f: impl FnOnce(&mut ExploreRound) -> std::result::Result<T, String>,
    ) -> T {
        let (value, round) = self
            .rounds
            .update(&"review".into(), &self.round.exploration.instance, f)
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

    fn path(&self, instance: &str) -> std::path::PathBuf {
        review_directory(&self.store).join(format!("{instance}.json"))
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
fn a_conclusion_completes_the_round_and_leaves_review_marks_alone() {
    let mut investigation = Investigation::new();
    let store = investigation.store.clone();
    let kickoff = investigation.request(None, None);
    assert!(investigation.submit(Investigation::update(&kickoff, 1)));
    let question = investigation.round.exploration.questions[0].clone();
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
        quiz: Vec::new(),
        quiz_empty_reason: Some("Nothing at whiteboard level.".into()),
    });
    let instance = investigation.round.exploration.instance.clone();

    let Submitted { applied, round } = investigation
        .rounds
        .submit(&"review".into(), &instance, &conclusion)
        .unwrap();

    assert!(applied);
    assert!(round.completion.is_some());
    assert_eq!(
        store.load(&"review".into(), b"policy.rs").unwrap(),
        prior_mark
    );
    let restored = investigation
        .rounds
        .round(&"review".into(), &instance)
        .unwrap()
        .unwrap();
    assert_eq!(restored, round);
    let Submitted { applied, .. } = investigation
        .rounds
        .submit(&"review".into(), &instance, &conclusion)
        .unwrap();
    assert!(!applied, "an identical retry changes nothing");
    let followup = round.exploration.clone().request(None, None).unwrap();
    investigation
        .rounds
        .update(&"review".into(), &instance, |round| {
            round.post(&followup).map_err(|error| error.to_string())
        })
        .unwrap();
    let Submitted { applied, .. } = investigation
        .rounds
        .submit(
            &"review".into(),
            &instance,
            &Investigation::update(&followup, 3),
        )
        .unwrap();
    assert!(applied, "the conversation remains open after concluding");
}

#[test]
fn clearing_one_unreadable_round_preserves_other_saved_rounds() {
    let fixture = Investigation::new();
    let unit = "review".into();
    let bad = fixture.round.exploration.instance.clone();
    let good = ExploreRound::new(Exploration::new(
        fixture.round.exploration.comparison.clone(),
    ));
    let good_instance = good.exploration.instance.clone();
    fixture.rounds.create(good).unwrap();
    let good_view = review_explore::ViewSave {
        instance: good_instance.clone(),
        ..fixture.view(1, "retained editor")
    };
    fixture.rounds.save_view(&unit, &good_view).unwrap();
    let bad_path = fixture.path(&bad);
    std::fs::write(&bad_path, b"invalid saved round").unwrap();

    fixture.rounds.clear_round(&unit, &bad).unwrap();

    assert!(!bad_path.exists());
    assert_eq!(
        fixture.rounds.history(&unit).unwrap().rounds,
        vec![good_instance.clone()]
    );
    assert!(
        fixture
            .rounds
            .round(&unit, &good_instance)
            .unwrap()
            .is_some()
    );
    assert_eq!(
        fixture.rounds.view(&unit, &good_instance).unwrap(),
        Some(good_view)
    );
}

#[test]
fn the_rounds_before_one_are_its_readable_predecessors_oldest_first() {
    let fixture = Investigation::new();
    let unit = "review".into();
    let first = fixture.round.exploration.instance.clone();
    let [unreadable, third, latest] = std::array::from_fn(|_| {
        let round = ExploreRound::new(Exploration::new(
            fixture.round.exploration.comparison.clone(),
        ));
        fixture.rounds.create(round).unwrap().exploration.instance
    });
    std::fs::write(fixture.path(&unreadable), b"invalid saved round").unwrap();

    let instances = |rounds: Vec<ExploreRound>| -> Vec<String> {
        rounds
            .into_iter()
            .map(|round| round.exploration.instance)
            .collect()
    };
    assert_eq!(
        instances(fixture.rounds.earlier(&unit, &latest).unwrap()),
        vec![first.clone(), third]
    );
    assert!(fixture.rounds.earlier(&unit, &first).unwrap().is_empty());
}

#[test]
fn repairing_unreadable_index_preserves_readable_rounds_and_views() {
    let fixture = Investigation::new();
    let unit = "review".into();
    let instance = fixture.round.exploration.instance.clone();
    fixture
        .rounds
        .save_view(&unit, &fixture.view(1, "draft"))
        .unwrap();
    let index = review_directory(&fixture.store).join("index.json");
    std::fs::write(&index, b"invalid index").unwrap();

    let repaired = fixture.rounds.repair_history(&unit).unwrap();

    assert_eq!(repaired.rounds, vec![instance.clone()]);
    assert!(!repaired.latest_editable);
    assert_eq!(fixture.rounds.history(&unit).unwrap(), repaired);
    assert!(fixture.rounds.round(&unit, &instance).unwrap().is_some());
    assert!(fixture.rounds.view(&unit, &instance).unwrap().is_some());
}

#[test]
fn accepted_output_survives_lost_ack_and_conflicting_retries_cannot_rewrite_it() {
    let mut fixture = Investigation::new();
    let request = fixture.request(None, None);
    let update = Investigation::update(&request, 1);
    assert!(fixture.submit(update.clone()));
    let revision = fixture.round.revision;
    assert!(!fixture.submit(update.clone()));
    assert_eq!(fixture.round.revision, revision);
    let mut conflict = update;
    conflict.next.as_mut().unwrap().text = "Replacement".into();
    let result = fixture.rounds.update(
        &"review".into(),
        &fixture.round.exploration.instance,
        |round| {
            round
                .exploration
                .submit(conflict)
                .map_err(|e| e.to_string())
        },
    );
    assert!(result.is_err());
    assert_eq!(
        fixture
            .rounds
            .round(&"review".into(), &request.instance)
            .unwrap()
            .unwrap(),
        fixture.round
    );
}

#[test]
fn one_view_keeps_latest_edits_without_rewriting_domain() {
    let fixture = Investigation::new();
    let path = fixture.path(&fixture.round.exploration.instance);
    let before = std::fs::read(&path).unwrap();
    for (sequence, text) in [(1, "First edits"), (2, "Latest edits")] {
        fixture
            .rounds
            .save_view(&"review".into(), &fixture.view(sequence, text))
            .unwrap();
    }
    fixture
        .rounds
        .save_view(&"review".into(), &fixture.view(1, "stale"))
        .unwrap();
    let view = fixture
        .rounds
        .view(&"review".into(), &fixture.round.exploration.instance)
        .unwrap()
        .unwrap();
    assert_eq!(view.sequence, 2);
    assert_eq!(view.state.tasks["conclusion"].text, "Latest edits");
    assert!(!path.with_extension("views").exists());
    assert_eq!(std::fs::read(path).unwrap(), before);
}

#[test]
fn new_round_keeps_previous_records_and_blocks_stale_domain_mutations() {
    let fixture = Investigation::new();
    let old = fixture.round.exploration.instance.clone();
    let new = ExploreRound::new(Exploration::new(
        fixture.round.exploration.comparison.clone(),
    ));
    fixture.rounds.create(new.clone()).unwrap();
    assert_eq!(
        fixture.rounds.history(&"review".into()).unwrap().rounds,
        vec![old.clone(), new.exploration.instance]
    );
    assert!(
        fixture
            .rounds
            .update(&"review".into(), &old, |_| Ok(()))
            .is_err()
    );
    assert_eq!(
        fixture
            .rounds
            .round(&"review".into(), &old)
            .unwrap()
            .unwrap(),
        fixture.round
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
        quiz: Vec::new(),
        quiz_empty_reason: Some("Nothing at whiteboard level.".into()),
    });
    fixture.submit(conclusion);
    fixture.mutate(|round| {
        round.completion = Some(review_explore::ReviewCompletion {
            request: turn.request.clone(),
            baseline: "checkpoint".into(),
        });
        Ok(())
    });
    let request = fixture
        .round
        .exploration
        .implementation("Only the edited task".into())
        .unwrap();
    fixture.mutate(|round| {
        round.last_agent_session = ConversationBinding::from_agent(&agent());
        round.authorize(&request).map_err(|e| e.to_string())
    });
    let id = DispatchId::Implementation {
        request: request.delivery.clone(),
        attempt: fixture.round.implementations[&request.delivery]
            .attempt
            .clone(),
    };
    (request, id)
}

#[test]
fn archived_attempt_keeps_authoritative_success_and_cancel_cannot_rewrite_it() {
    let mut fixture = Investigation::new();
    let (request, id) = implementation(&mut fixture);
    fixture.mutate(|round| {
        round
            .begin_dispatch(&id, &agent())
            .map_err(|e| e.to_string())
    });
    let round = fixture
        .rounds
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
        round.implementations[&request.delivery].state,
        DispatchState::Attempting,
        "A second observer's cancellation cannot cancel the first observer's external attempt"
    );
    fixture
        .rounds
        .create(ExploreRound::new(Exploration::new(
            fixture.round.exploration.comparison.clone(),
        )))
        .unwrap();
    for state in [DispatchState::Delivered, DispatchState::Cancelled] {
        let round = fixture
            .rounds
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
            round.implementations[&request.delivery].state,
            DispatchState::Delivered
        );
        assert_eq!(
            round.implementations[&request.delivery].request.text,
            "Only the edited task"
        );
    }
}

#[test]
fn superseded_attempt_cannot_change_new_attempt_or_authorized_payload() {
    let mut fixture = Investigation::new();
    let (request, old) = implementation(&mut fixture);
    fixture.mutate(|round| round.authorize(&request).map_err(|e| e.to_string()));
    let round = fixture
        .rounds
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
        round.implementations[&request.delivery].state,
        DispatchState::Queued
    );
    let mut changed = request.clone();
    changed.text = "Expanded scope".into();
    assert!(
        fixture
            .rounds
            .update(&"review".into(), &request.instance, |round| round
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
        attempt: fixture.round.turns[&old.request].attempt.clone(),
    };
    fixture.mutate(|round| {
        round.exploration.cancel();
        Ok(())
    });
    assert!(
        fixture
            .rounds
            .update(&"review".into(), &old.instance, |round| round
                .begin_dispatch(&id, &agent())
                .map_err(|e| e.to_string()))
            .is_err()
    );
    let new = fixture.request(None, None);
    assert!(
        fixture
            .rounds
            .update(&"review".into(), &old.instance, |round| round
                .post(&old)
                .map_err(|e| e.to_string()))
            .is_err()
    );
    let saved = fixture
        .rounds
        .round(&"review".into(), &old.instance)
        .unwrap()
        .unwrap();
    assert_eq!(saved.exploration.pending_request(), Some(&new));
    assert_eq!(saved.exploration.retry_request(), Some(&new));
}

#[test]
fn editor_sequence_conflict_and_wrong_review_keep_original_bytes() {
    let fixture = Investigation::new();
    let view = fixture.view(1, "Original text");
    fixture.rounds.save_view(&"review".into(), &view).unwrap();
    assert!(
        fixture
            .rounds
            .save_view(&"review".into(), &fixture.view(1, "Conflicting text"))
            .is_err()
    );
    assert!(
        fixture
            .rounds
            .save_view(&"different-review".into(), &view)
            .is_err()
    );
    assert_eq!(
        fixture
            .rounds
            .view(&"review".into(), &view.instance)
            .unwrap()
            .unwrap(),
        view
    );
}
