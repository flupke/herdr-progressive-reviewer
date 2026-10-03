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
#[path = "page.tests.rs"]
mod page;
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
    /// Where a reopened session records the prompts it sends.
    turns: Option<TurnLog>,
    /// The stage of the round the session publishes for the Explore page.
    page: review_explore_page::RoundFeed,
    delivered: usize,
    threads: review_thread_service::Worker,
    files: Box<dyn ReviewRepositoryFixture>,
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
        let publisher = review_explore_page::RoundPublisher::default();
        let page = publisher.subscribe();
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
                turns: None,
                page: publisher,
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
            turns: None,
            page,
            delivered: 0,
            threads,
            files,
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
        let publisher = review_explore_page::RoundPublisher::default();
        self.page = publisher.subscribe();
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
            turns: self.turns.clone(),
            page: publisher,
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

    /// Submit as the agent and acknowledge each committed round as the UI does.
    fn submit(&mut self, access: &str, operation: Operation) -> Result<Response, String> {
        self.submit_acknowledged(access, operation, true)
    }

    /// Submit as the agent; without `acknowledge` the UI drops each committed round.
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
                self.exploration = Some(committed.round.exploration.clone());
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

    /// Adopt a restored round as the UI does: its interrupted turn waits for Retry.
    fn adopt(&mut self, restored: &ui_events::ExploreRestored) {
        let round = restored.result.as_ref().unwrap().as_ref().unwrap();
        let mut exploration = round.exploration.clone();
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

    fn round_path(&self) -> std::path::PathBuf {
        self.review_directory()
            .join(format!("{}.json", self.exploration().instance))
    }

    /// Change the saved round as an earlier, interrupted process left it.
    fn damage(&self, change: impl FnOnce(&mut ExploreRound)) {
        self.store
            .lock_explore(&self.unit)
            .unwrap()
            .update_round(&self.exploration().instance, |round| {
                change(round);
                Ok(())
            })
            .unwrap();
    }

    fn history(&self) -> review_explore::ExploreHistory {
        self.store.load_explore_history(&self.unit).unwrap()
    }

    fn saved(&self) -> ExploreRound {
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
    assert_eq!(marks.answer.as_ref(), Some(&answer));
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

#[test]
fn a_vision_session_records_each_prompt_it_sent_as_a_numbered_turn() {
    let mut harness = Harness::start();
    let directory = tempfile::tempdir().unwrap();
    harness.turns = Some(TurnLog::open(directory.path().to_owned()).unwrap());
    harness.reopen();
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    let turn = |number: u32| -> serde_json::Value {
        let path = directory.path().join(format!("turn-{number:06}.json"));
        assert!(review_test_support::eventually(
            Duration::from_secs(10),
            || path.exists()
        ));
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    };

    let kickoff = turn(1);
    assert_eq!(kickoff["turn"], 1);
    assert_eq!(kickoff["kind"], "kickoff");
    assert_eq!(kickoff["delivered"], true);
    assert_eq!(kickoff["access"], access);
    assert_eq!(kickoff["request"], first.request);
    assert_eq!(kickoff["round"], first.instance);
    assert_eq!(
        kickoff["text"],
        harness.agents.prompts().last().unwrap().text
    );
    assert!(
        kickoff["unreviewed"]
            .as_str()
            .unwrap()
            .contains("Unreviewed diffs:")
    );

    assert!(applied(harness.submit(&access, question(&first, 1))));
    let (answer, access) = harness.answer("Keep it.");
    let wakeup = turn(2);
    assert_eq!(wakeup["kind"], "wakeup");
    assert_eq!(wakeup["access"], access);
    assert_eq!(wakeup["answer"]["id"], answer.answer.as_ref().unwrap().id);
    assert_eq!(wakeup["answer"]["option"]["id"], "keep");
    assert_eq!(wakeup["answer"]["question"]["id"], "q1");
    assert_eq!(wakeup["answer"]["text"], "Keep it.");

    // A reopened reviewer continues after the highest turn, whatever is missing.
    std::fs::remove_file(directory.path().join("turn-000001.json")).unwrap();
    std::fs::write(directory.path().join("turn-000009.partial"), b"{").unwrap();
    let reopened = TurnLog::open(directory.path().to_owned()).unwrap();
    reopened.record(
        &turn_log::SentTurn::implement("Do the tasks".into()),
        &Ok(()),
    );
    assert_eq!(turn(3)["kind"], "implement");
    assert!(turn(3).get("access").is_none());
}

#[test]
fn each_prompt_points_at_numbered_diffs_of_what_is_still_unreviewed() {
    use std::os::unix::fs::PermissionsExt;
    let mut harness = Harness::start();
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    let directory = |prompt: &str| -> std::path::PathBuf {
        prompt
            .lines()
            .find_map(|line| line.strip_prefix("Unreviewed diffs: "))
            .and_then(|line| line.split_once("/<repository path>"))
            .expect("the prompt names the diffs")
            .0
            .into()
    };
    let kickoff = harness.agents.prompts().last().unwrap().text.clone();
    let diffs = directory(&kickoff);

    assert_eq!(
        std::fs::metadata(&diffs).unwrap().permissions().mode() & 0o777,
        0o700
    );
    let diff = std::fs::read_to_string(diffs.join("reviewed.rs")).unwrap();
    assert!(
        diff.starts_with("reviewed.rs: unreviewed lines\n"),
        "{diff}"
    );
    assert!(
        diff.ends_with("old new\n      1 + pub fn reviewed() {}\n"),
        "{diff}"
    );

    // Each prompt has a directory of its own; the previous one is removed.
    assert!(applied(harness.submit(&access, question(&first, 1))));
    let (request, access) = harness.answer("Keep it.");
    let wakeup = harness.agents.prompts().last().unwrap().text.clone();
    let next = directory(&wakeup);
    assert_ne!(next, diffs);
    assert!(!diffs.exists() && next.join("reviewed.rs").exists());

    // The answer's turn marks the line: nothing is left to show.
    let Operation::SubmitQuestion(mut update) = question(&request, 2) else {
        unreachable!("a question");
    };
    update.reviewed = vec![
        serde_json::from_value(serde_json::json!({
            "path": "reviewed.rs", "side": "new", "lines": null
        }))
        .unwrap(),
    ];
    assert!(applied(
        harness.submit(&access, Operation::SubmitQuestion(update))
    ));
    harness.answer("Keep it again.");
    let wakeup = harness.agents.prompts().last().unwrap().text.clone();
    assert!(wakeup.contains("Unreviewed diffs: none"), "{wakeup}");
    assert!(!next.exists());
}

#[test]
fn a_prompt_whose_diffs_cannot_be_written_is_not_sent_and_says_why() {
    let mut harness = Harness::start();
    harness.capture();
    let first = harness.request(None);
    // The repository can no longer be read.
    std::fs::remove_dir_all(harness.files.root().join(".git")).unwrap();

    harness
        .session
        .handle(Input::Command(Command::Turn(Box::new(first.clone()))));

    let toast = harness.next::<ui_events::ToastRequested>();
    assert!(
        toast
            .text
            .starts_with("Explore could not write the unreviewed diffs"),
        "{}",
        toast.text
    );
    let finished = harness.next::<ui_events::ExploreFinished>();
    assert_eq!(finished.request, first.request);
    assert!(finished.result.is_err());
    assert!(harness.agents.prompts().is_empty());
}

#[test]
fn each_not_relevant_mark_keeps_its_reason_and_test_with_the_lines_it_marked() {
    let mut harness = Harness::start();
    harness.files.write("reviewed.rs", b"one\ntwo\nthree\n");
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    let Operation::SubmitQuestion(mut update) = question(&first, 1) else {
        unreachable!("a question");
    };
    let marks: Vec<review_explore::NotRelevantMark> = serde_json::from_value(serde_json::json!([
        {"path": "reviewed.rs", "side": "new", "lines": {"first_line": 1, "last_line": 1},
            "reason": "follows_code"},
        {"path": "reviewed.rs", "side": "new", "lines": {"first_line": 2, "last_line": 3},
            "reason": "tested_mechanics",
            "test": {"path": "reviewed.rs", "lines": {"first_line": 1, "last_line": 1}}},
    ]))
    .unwrap();
    update.not_relevant = marks.clone();
    assert!(applied(
        harness.submit(&access, Operation::SubmitQuestion(update))
    ));

    harness.answer("Keep it.");

    assert_eq!(harness.saved().marks[&first.request].not_relevant, marks);
}

#[test]
fn a_questions_marks_wait_for_its_answer() {
    let mut harness = Harness::start();
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    let Operation::SubmitQuestion(mut update) = question(&first, 1) else {
        unreachable!("a question");
    };
    update.not_relevant = vec![
        serde_json::from_value(serde_json::json!({
            "path": "reviewed.rs", "side": "new", "lines": null, "reason": "follows_code"
        }))
        .unwrap(),
    ];
    let marked =
        |harness: &Harness| match harness.store.load(&harness.unit, b"reviewed.rs").unwrap() {
            review_store::LoadResult::Reviewed(record) => Some(record.author),
            _ => None,
        };

    assert!(applied(
        harness.submit(&access, Operation::SubmitQuestion(update))
    ));

    // Accepting the question marks nothing: progress moves when the reviewer answers.
    assert!(harness.saved().marks.is_empty());
    assert_eq!(marked(&harness), None);

    let (request, _) = harness.answer("Keep it.");

    let answer = request.answer.unwrap().id;
    let saved = harness.saved();
    let marks = &saved.marks[&first.request];
    assert_eq!(marks.answer.as_ref(), Some(&answer));
    assert_eq!(marks.counts().not_relevant_lines, 1);
    assert!(marks.reviewed.is_empty());
    assert_eq!(
        marked(&harness),
        Some(review_types::MarkAuthor::Explore { answer })
    );
}

#[test]
fn a_round_keeps_its_challenger_from_the_kickoff() {
    let mut harness = Harness::start();
    harness.capture();
    harness.exploration.as_mut().unwrap().challenger = true;
    let first = harness.request(None);

    let access = harness.turn(&first);

    assert!(harness.saved().exploration.challenger);
    assert!(applied(harness.submit(&access, question(&first, 1))));
    let (answered, _) = harness.answer("Keep it.");
    assert!(answered.challenger && harness.saved().exploration.challenger);
}

#[test]
fn a_reset_round_is_closed_and_reopening_shows_the_start_screen() {
    let mut harness = Harness::start();
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    assert!(applied(harness.submit(&access, question(&first, 1))));
    let (answered, access) = harness.answer("Keep it.");

    harness.session.handle(Input::Command(Command::Reset));

    assert!(harness.history().closed);
    let late = harness.submit(&access, question(&answered, 2));
    assert!(late.is_err(), "the agent's late question reaches no round");
    assert_eq!(
        harness.saved().exploration.answers.len(),
        1,
        "its record stays"
    );
    assert!(harness.reopen().result.unwrap().is_none());

    harness.capture();
    let kickoff = harness.request(None);
    harness.turn(&kickoff);
    let history = harness.history();
    assert!(!history.closed);
    assert_eq!(history.restorable(), Some(kickoff.instance.as_str()));
}

#[test]
fn a_new_rounds_kickoff_lists_the_earlier_rounds_decisions_without_cancelled_answers() {
    let mut harness = Harness::start();
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    let opening = harness.agents.prompts().last().unwrap().text.clone();
    assert!(applied(harness.submit(&access, question(&first, 1))));
    let (kept, access) = harness.answer("Keep it, round one.");
    assert!(applied(harness.submit(&access, question(&kept, 2))));
    let (withdrawn, _) = harness.answer("Withdrawn comment.");
    let withdrawn = withdrawn.answer.unwrap().id;
    harness
        .session
        .handle(Input::Command(Command::CancelAnswer(withdrawn.clone())));
    assert!(
        harness
            .next::<ui_events::ExploreAnswerCancelled>()
            .result
            .is_ok()
    );
    harness.session.handle(Input::Command(Command::Reset));

    harness.capture();
    let kickoff = harness.request(None);
    harness.turn(&kickoff);
    let prompt = harness.agents.prompts().last().unwrap().text.clone();

    let kept = kept.answer.unwrap();
    assert!(prompt.contains(&kept.id), "{prompt}");
    assert!(prompt.contains("> Keep it, round one.\n"), "{prompt}");
    assert!(!prompt.contains(&withdrawn) && !prompt.contains("Withdrawn comment."));
    assert!(
        !opening.contains("\n> "),
        "the first round has no earlier decisions"
    );
}

#[test]
fn resetting_a_latest_round_left_read_only_closes_it() {
    let mut harness = Harness::start();
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    assert!(applied(harness.submit(&access, question(&first, 1))));
    let mut history = harness.history();
    history.latest_editable = false;
    harness
        .store
        .lock_explore(&harness.unit)
        .unwrap()
        .save_history(&history)
        .unwrap();
    assert!(harness.reopen().historical);

    harness.session.handle(Input::Command(Command::Reset));

    assert!(harness.history().closed);
    assert!(harness.reopen().result.unwrap().is_none());
}
