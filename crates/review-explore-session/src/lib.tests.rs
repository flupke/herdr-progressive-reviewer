//! Explore sessions over a temporary repository and store, prompting in-memory agents.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use component_core::EventEnvelope;
use herdr_client::memory::InMemoryAgents;
use herdr_client::protocol::{Agent, AgentSession, AgentStatus, PaneId, TabId, WorkspaceId};
use review_explore::{
    AnswerInput, ClassificationState, CoverageLedger, CoverageUnit, Exploration, GapQuery,
    InterviewUpdate, JevMode, Significance, SignificanceClassifier, SignificancePlan,
    SignificanceResult, TurnRequest,
};
use review_mcp::{Operation, Request, Response};
use review_repository::repository::RepoType;
use review_test_support::{
    ReviewRepositoryFixture, complete_repository_snapshot, repository_fixture,
};
use tempfile::TempDir;

use super::*;

const PANE: &str = "agent-pane";
const RUBRIC: &str = "test-rubric";

/// One session driven directly, as the reviewer's worker drives it.
struct Harness {
    session: ExploreSession,
    events: crossbeam_channel::Receiver<EventEnvelope>,
    inbox: mpsc::Receiver<Input>,
    agents: InMemoryAgents,
    store: ReviewStore,
    exploration: Option<Exploration>,
    delivered: usize,
    _threads: review_thread_service::Worker,
    _files: Box<dyn ReviewRepositoryFixture>,
    _state: TempDir,
}

impl Harness {
    fn start(exclusion: ExclusionPolicy) -> Self {
        let files = repository_fixture(RepoType::Git);
        files.write("reviewed.rs", b"pub fn reviewed() {}\n");
        let state = tempfile::tempdir().unwrap();
        let repository = Repository::discover(files.root())
            .unwrap()
            .with_state_root(state.path());
        let store = ReviewStore::open(state.path(), repository.root()).unwrap();
        let agents = InMemoryAgents::default();
        agents.upsert_agent(agent());
        let target = AgentTarget::new(workspace(), Some(PaneId(PANE.into())));
        // The thread service owns prompt delivery; it has no MCP listener here.
        let threads = review_thread_service::Worker::start(
            store.clone(),
            agents.clone(),
            target.clone(),
            Err("No MCP listener in this test".into()),
            |_| Err("No MCP listener in this test".into()),
            |_| {},
        );
        let (event_sender, events) = crossbeam_channel::unbounded();
        let (inbox_sender, inbox) = mpsc::channel();
        let unit = complete_repository_snapshot(&repository)
            .identity
            .review_unit()
            .clone();
        let mut session = ExploreSession::new(Collaborators {
            repository,
            store: store.clone(),
            agents: Arc::new(agents.clone()),
            target,
            prompts: threads.prompt_sender(),
            exclusion,
            events: ApplicationEventSender::new(event_sender),
            inbox: Inbox::new(move |input| {
                let _ = inbox_sender.send(input);
            }),
        });
        session.checkpoint_changed(&unit);
        let harness = Self {
            session,
            events,
            inbox,
            agents,
            store,
            exploration: None,
            delivered: 0,
            _threads: threads,
            _files: files,
            _state: state,
        };
        let restored = harness.next::<ui_events::ExploreRestored>();
        assert!(matches!(restored.result, Ok(None)));
        harness
    }

    fn next<E: component_core::ApplicationEvent + Clone>(&self) -> E {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let event = self.events.recv_timeout(remaining).unwrap();
            if let Some(event) = event.downcast_ref::<E>() {
                return event.clone();
            }
        }
    }

    fn capture(&mut self) {
        self.session.handle(Input::Command(Command::Start));
        let comparison = self.next::<ui_events::ExploreCaptured>().result.unwrap();
        self.exploration = Some(Exploration::new(comparison));
    }

    fn exploration(&self) -> &Exploration {
        self.exploration.as_ref().unwrap()
    }

    /// The reviewer's next request, answering the latest question when `answer` is set.
    fn request(&mut self, answer: Option<AnswerInput>) -> TurnRequest {
        let exploration = self.exploration.as_mut().unwrap();
        let question = exploration.questions.last().cloned();
        exploration.request(answer, question.as_ref()).unwrap()
    }

    /// Post a turn and return the access value its delivered prompt grants.
    fn turn(&mut self, request: &TurnRequest) -> String {
        self.session
            .handle(Input::Command(Command::Turn(Box::new(request.clone()))));
        assert!(self.next::<ui_events::ExplorePosted>().result.is_ok());
        let prompt = self.delivered_prompt();
        assert!(prompt.contains(&format!("Explore request: {}\n", request.request)));
        if let Some(answer) = &request.answer {
            assert!(prompt.contains(&format!("Answer ID: {}\n", answer.id)));
        }
        prompt
            .lines()
            .find_map(|line| line.strip_prefix("Explore review access: "))
            .expect("prompt grants Explore access")
            .to_owned()
    }

    fn delivered_prompt(&mut self) -> String {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let prompts = self.agents.prompts();
            if let Some(prompt) = prompts.get(self.delivered) {
                self.delivered += 1;
                assert_eq!(prompt.pane_id, PaneId(PANE.into()));
                return prompt.text.clone();
            }
            assert!(
                self.inbox.try_recv().is_err(),
                "the prompt was not delivered"
            );
            assert!(Instant::now() < deadline, "no prompt was delivered");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Submit as the agent and acknowledge each committed pass as the UI does.
    fn submit(&mut self, access: &str, operation: Operation) -> Result<Response, String> {
        let (request, mut pending) = Request::new(access.to_owned(), operation);
        self.session.handle(Input::Submission(Box::new(request)));
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Ok(result) = pending.try_recv() {
                return result;
            }
            assert!(Instant::now() < deadline, "the session did not respond");
            if let Ok(event) = self.events.recv_timeout(Duration::from_millis(10))
                && let Some(committed) = event.downcast_ref::<ui_events::ExploreCommitted>()
            {
                self.exploration = Some(committed.pass.exploration.clone());
                committed.response.send(Ok(committed.applied)).unwrap();
            }
        }
    }

    fn saved(&self) -> ExplorePass {
        let exploration = self.exploration();
        self.store
            .load_explore(
                &exploration.comparison.checkpoint.review_unit,
                &exploration.instance,
            )
            .unwrap()
            .unwrap()
    }
}

fn workspace() -> WorkspaceId {
    WorkspaceId("workspace".into())
}

fn agent() -> Agent {
    Agent {
        pane_id: PaneId(PANE.into()),
        tab_id: TabId("tab".into()),
        workspace_id: workspace(),
        name: None,
        display_agent: None,
        agent: Some("codex".into()),
        agent_status: AgentStatus::Idle,
        agent_session: Some(AgentSession {
            source: "herdr:codex".into(),
            agent: "codex".into(),
            kind: "id".into(),
            value: "conversation".into(),
        }),
        cwd: None,
    }
}

fn question(request: &TurnRequest, version: u32) -> Operation {
    let interpretation = request.answer.as_ref().map(|answer| {
        serde_json::json!({
            "answer": answer.id, "status": "accepted", "recap": "Keep the policy.", "follow_ups": []
        })
    });
    let update: InterviewUpdate = serde_json::from_value(serde_json::json!({
        "instance": request.instance, "request": request.request, "checkpoint": request.checkpoint,
        "interpretation": interpretation, "reply": {"text": "I checked the policy.", "evidence": []},
        "topics": [{"id": format!("topic{version}"), "title": "Policy", "entries": [{"path": "reviewed.rs", "side": "new", "lines": null}], "status": "open"}],
        "next": {"id": format!("q{version}"), "version": 1, "topic": format!("topic{version}"), "text": "Keep the policy?",
            "rationale": null, "visual": null,
            "alternatives": [{"id": "keep", "text": "Keep it", "outcome": "accepted"}, {"id": "change", "text": "Change it", "outcome": "needs_follow_up"}],
            "evidence": [{"path": "reviewed.rs", "side": "new", "lines": {"first_line": 1, "last_line": 1}, "notes": "Implements the policy"}]},
        "conclusion": null, "limitations": [], "findings": []
    }))
    .unwrap();
    Operation::SubmitQuestion(Box::new(update))
}

fn applied(result: Result<Response, String>) -> bool {
    match result {
        Ok(Response::Explore { applied, .. }) => applied,
        other => panic!("unexpected Explore response: {other:?}"),
    }
}

#[test]
fn a_question_and_its_answer_reach_the_agent_and_are_saved() {
    let mut harness = Harness::start(ExclusionPolicy::disabled());
    harness.capture();

    let first = harness.request(None);
    let access = harness.turn(&first);
    assert!(applied(harness.submit(&access, question(&first, 1))));
    assert!(!applied(harness.submit(&access, question(&first, 1))));

    let answer = harness.request(Some(AnswerInput {
        option: Some("keep".into()),
        text: "Keep it, with a regression test.".into(),
        ..AnswerInput::default()
    }));
    let renewed = harness.turn(&answer);
    assert_ne!(renewed, access, "each prompt grants its own access");
    let stale = harness.submit(&access, question(&answer, 2)).unwrap_err();
    assert!(stale.contains("Obsolete Explore access"), "{stale}");
    assert!(applied(harness.submit(&renewed, question(&answer, 2))));

    let saved = harness.saved();
    assert_eq!(saved.exploration.answers.len(), 1);
    assert_eq!(saved.exploration.questions.len(), 2);
    assert!(saved.last_agent_session.unwrap().matches(&agent()));
    assert_eq!(harness.agents.prompts().len(), 2);
}

/// Excludes every changed line of the comparison as insignificant.
struct EverythingInsignificant;

impl SignificanceClassifier for EverythingInsignificant {
    fn rubric(&self) -> &str {
        RUBRIC
    }

    fn plan(&self, comparison: &Comparison, classified: &dyn Fn(&str) -> bool) -> SignificancePlan {
        let units: Vec<_> = CoverageLedger::new(comparison)
            .inventory()
            .units
            .iter()
            .filter(|unit| matches!(unit, CoverageUnit::Lines { .. }))
            .cloned()
            .collect();
        let pending = !classified("everything");
        SignificancePlan::new(1, move |record| {
            !pending
                || record(SignificanceResult {
                    id: "everything".into(),
                    units,
                    outcome: Significance::Insignificant,
                    model: None,
                    rubric: RUBRIC.into(),
                    criterion: String::new(),
                    input_references: Vec::new(),
                    omissions: Vec::new(),
                    probabilities: std::collections::BTreeMap::new(),
                    confidence: None,
                    error: None,
                })
        })
    }
}

#[test]
fn an_enabled_exclusion_policy_classifies_the_pass_and_answers_gaps_in_enabled_mode() {
    let mut harness = Harness::start(ExclusionPolicy::enabled(Arc::new(EverythingInsignificant)));
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);

    let deadline = Instant::now() + Duration::from_secs(10);
    while !harness
        .saved()
        .coverage
        .classification_progress()
        .is_some_and(|progress| matches!(progress.state, ClassificationState::Finished { .. }))
    {
        assert!(Instant::now() < deadline, "classification did not finish");
        std::thread::sleep(Duration::from_millis(10));
    }
    let pass = harness.saved();
    assert!(!pass.coverage.unexplored_exclusions().is_empty());
    assert!(!pass.coverage.needs_classification(RUBRIC, pass.revision));

    let gaps = |mode| {
        Operation::GetCoverageGaps(Box::new(GapQuery {
            instance: pass.exploration.instance.clone(),
            checkpoint: pass.exploration.comparison.checkpoint.clone(),
            revision: pass.coverage.revision(),
            mode,
            path_prefix: None,
            cursor: None,
            limit: None,
        }))
    };
    let stale = harness
        .submit(&access, gaps(JevMode::Disabled))
        .unwrap_err();
    assert!(stale.contains("Jev policy changed"), "{stale}");
    assert!(matches!(
        harness.submit(&access, gaps(JevMode::Enabled)),
        Ok(Response::CoverageGaps(_))
    ));
}
