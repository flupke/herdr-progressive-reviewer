//! Explore sessions over a temporary repository and store, prompting in-memory agents.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use component_core::EventEnvelope;
use herdr_client::memory::InMemoryAgents;
use herdr_client::protocol::{Agent, AgentSession, AgentStatus, PaneId, TabId, WorkspaceId};
use review_explore::{AnswerInput, Exploration, InterviewUpdate, TurnRequest};
use review_mcp::{Operation, Request, Response};
use review_repository::repository::RepoType;
use review_test_support::{
    ReviewRepositoryFixture, complete_repository_snapshot, repository_fixture,
};
use tempfile::TempDir;

use super::*;

const PANE: &str = "agent-pane";
const CONCLUSION: &str = "Keep the policy.";

#[path = "cancel.tests.rs"]
mod cancel;
#[path = "recovery.tests.rs"]
mod recovery;

/// One session driven directly, as the reviewer's worker drives it.
struct Harness {
    session: ExploreSession,
    events: crossbeam_channel::Receiver<EventEnvelope>,
    event_sender: crossbeam_channel::Sender<EventEnvelope>,
    inbox: mpsc::Receiver<Input>,
    inbox_sender: mpsc::Sender<Input>,
    agents: InMemoryAgents,
    store: ReviewStore,
    repository: Repository,
    unit: ReviewUnit,
    exploration: Option<Exploration>,
    delivered: usize,
    threads: review_thread_service::Worker,
    _files: Box<dyn ReviewRepositoryFixture>,
    _state: TempDir,
}

impl Harness {
    fn start() -> Self {
        let files = repository_fixture(RepoType::Git);
        files.write("reviewed.rs", b"pub fn reviewed() {}\n");
        let state = tempfile::tempdir().unwrap();
        let repository = Repository::discover(files.root())
            .unwrap()
            .with_state_root(state.path());
        let store = ReviewStore::open(state.path(), repository.root()).unwrap();
        let agents = InMemoryAgents::default();
        agents.upsert_agent(agent());
        // The thread service owns prompt delivery; it has no MCP listener here.
        let threads = review_thread_service::Worker::start(
            store.clone(),
            agents.clone(),
            target(),
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
        let mut harness = Self {
            session: ExploreSession::new(Collaborators {
                repository: repository.clone(),
                store: store.clone(),
                tracker: Arc::new(review_state::ReviewTracker::new(
                    repository.clone(),
                    store.clone(),
                )),
                agents: Arc::new(agents.clone()),
                target: target(),
                prompts: threads.prompt_sender(),
                events: ApplicationEventSender::new(event_sender.clone()),
                inbox: Inbox::new(|_| {}),
            }),
            events,
            event_sender,
            inbox,
            inbox_sender,
            agents,
            store,
            repository,
            unit,
            exploration: None,
            delivered: 0,
            threads,
            _files: files,
            _state: state,
        };
        let restored = harness.reopen();
        assert!(matches!(restored.result, Ok(None)));
        harness
    }

    /// Replace the session as a restarted reviewer would, keeping only saved state,
    /// the agents and the prompts already delivered.
    fn reopen(&mut self) -> ui_events::ExploreRestored {
        while self.events.try_recv().is_ok() {}
        let inbox = self.inbox_sender.clone();
        self.session = ExploreSession::new(Collaborators {
            repository: self.repository.clone(),
            store: self.store.clone(),
            tracker: Arc::new(review_state::ReviewTracker::new(
                self.repository.clone(),
                self.store.clone(),
            )),
            agents: Arc::new(self.agents.clone()),
            target: target(),
            prompts: self.threads.prompt_sender(),
            events: ApplicationEventSender::new(self.event_sender.clone()),
            inbox: Inbox::new(move |input| {
                let _ = inbox.send(input);
            }),
        });
        let unit = self.unit.clone();
        self.session.checkpoint_changed(&unit);
        self.next::<ui_events::ExploreRestored>()
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
        self.submit_acknowledged(access, operation, true)
    }

    /// Submit as the agent; without `acknowledge` the UI drops each committed pass.
    fn submit_acknowledged(
        &mut self,
        access: &str,
        operation: Operation,
        acknowledge: bool,
    ) -> Result<Response, String> {
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
                && acknowledge
            {
                self.exploration = Some(committed.pass.exploration.clone());
                committed.response.send(Ok(committed.applied)).unwrap();
            }
        }
    }

    /// Answer the latest question as the reviewer, then post it.
    fn answer(&mut self, text: &str) -> (TurnRequest, String) {
        let request = self.request(Some(AnswerInput {
            option: Some("keep".into()),
            text: text.into(),
            ..AnswerInput::default()
        }));
        let access = self.turn(&request);
        (request, access)
    }

    /// Ask the first question, answer it and let the agent conclude.
    fn conclude(&mut self) -> (TurnRequest, String) {
        self.capture();
        let first = self.request(None);
        let access = self.turn(&first);
        assert!(applied(self.submit(&access, question(&first, 1))));
        let (request, access) = self.answer("Keep it.");
        assert!(applied(
            self.submit(&access, conclusion(&request, CONCLUSION))
        ));
        (request, access)
    }

    /// Adopt a restored pass as the UI does: its interrupted turn waits for Retry.
    fn adopt(&mut self, restored: &ui_events::ExploreRestored) {
        let pass = restored.result.as_ref().unwrap().as_ref().unwrap();
        let mut exploration = pass.exploration.clone();
        exploration.pause_delivery();
        self.exploration = Some(exploration);
    }

    fn review_directory(&self) -> std::path::PathBuf {
        std::fs::read_dir(self.store.explore_directory())
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path()
    }

    fn pass_path(&self) -> std::path::PathBuf {
        self.review_directory()
            .join(format!("{}.json", self.exploration().instance))
    }

    /// Change the saved pass as an earlier, interrupted process left it.
    fn damage(&self, change: impl FnOnce(&mut ExplorePass)) {
        self.store
            .lock_explore(&self.unit)
            .unwrap()
            .update_pass(&self.exploration().instance, |pass| {
                change(pass);
                Ok(())
            })
            .unwrap();
    }

    fn history(&self) -> review_explore::ExploreHistory {
        self.store.load_explore_history(&self.unit).unwrap()
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

fn target() -> AgentTarget {
    AgentTarget::new(workspace(), Some(PaneId(PANE.into())))
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

fn conclusion(request: &TurnRequest, summary: &str) -> Operation {
    let submission = serde_json::from_value(serde_json::json!({
        "review": "", "instance": request.instance, "request": request.request,
        "checkpoint": request.checkpoint,
        "interpretation": request.answer.as_ref().map(|answer| serde_json::json!({
            "answer": answer.id, "status": "accepted", "recap": "Keep the policy.", "follow_ups": []
        })),
        "summary": summary, "to_be_implemented": "Add a regression test.", "future_work": "Later."
    }))
    .unwrap();
    Operation::SubmitConclusion(Box::new(submission))
}

fn applied(result: Result<Response, String>) -> bool {
    match result {
        Ok(Response::Explore { applied, .. }) => applied,
        other => panic!("unexpected Explore response: {other:?}"),
    }
}

#[test]
fn a_question_and_its_answer_reach_the_agent_and_are_saved() {
    let mut harness = Harness::start();
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

/// A conclusion that marks the only changed line reviewed after `request`'s answer.
fn marking_conclusion(request: &TurnRequest) -> Operation {
    let Operation::SubmitConclusion(mut submission) = conclusion(request, CONCLUSION) else {
        unreachable!("a conclusion");
    };
    submission.reviewed = vec![
        serde_json::from_value(serde_json::json!({
            "path": "reviewed.rs", "side": "new", "lines": {"first_line": 1, "last_line": 1}
        }))
        .unwrap(),
    ];
    Operation::SubmitConclusion(submission)
}

#[test]
fn the_lines_an_answer_settled_are_marked_reviewed_by_explore() {
    let mut harness = Harness::start();
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    assert!(applied(harness.submit(&access, question(&first, 1))));
    let (request, access) = harness.answer("Keep it.");

    assert!(applied(
        harness.submit(&access, marking_conclusion(&request))
    ));

    let answer = request.answer.unwrap().id;
    let saved = harness.saved();
    let marks = &saved.marks[&request.request];
    assert_eq!(marks.answer, answer);
    assert_eq!(marks.counts().reviewed_lines, 1);
    assert_eq!(marks.problem, None);
    let record = harness.store.load(&harness.unit, b"reviewed.rs").unwrap();
    let review_store::LoadResult::Reviewed(record) = record else {
        panic!("the line was marked");
    };
    assert_eq!(record.author, review_types::MarkAuthor::Explore { answer });
}

#[test]
fn the_kickoff_turn_cannot_mark_lines() {
    let mut harness = Harness::start();
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    let Operation::SubmitQuestion(mut update) = question(&first, 1) else {
        unreachable!("a question");
    };
    update.reviewed = vec![
        serde_json::from_value(serde_json::json!({
            "path": "reviewed.rs", "side": "new", "lines": null
        }))
        .unwrap(),
    ];

    let error = harness
        .submit(&access, Operation::SubmitQuestion(update))
        .unwrap_err();

    assert!(error.contains("follow a human answer"), "{error}");
}
