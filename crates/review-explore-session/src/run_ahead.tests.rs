//! Run-ahead in the session, with a fake agent host: which forks start, what their prompts
//! carry, what a fork may submit, and when forks are discarded.

use std::collections::HashMap;
use std::sync::Mutex;

use agent_fork::{ForkCommand, ProcessStamp};
use review_explore_page::RoundStage;
use review_explore_round_settings::RunAhead;
use review_run_ahead::{
    Continuation, DiscardReason, ForkEnd, ForkHost, ForkPoint, ForkRecord, ForkStart, ForkTrace,
    PaneWatch, PlainReason, RoundForks, StatusReport, SwitchFailure, SwitchTo, TurnPath,
};

use super::*;

/// An agent host that starts no process: it records the forks it is asked to start and
/// discard, and lets a test end them, report the agent's status and move its session.
#[derive(Clone, Default)]
pub(super) struct FakeForks(Arc<Mutex<FakeHost>>);

#[derive(Default)]
struct FakeHost {
    /// The last conversation entry of the agent's session, as it is now.
    entry: Option<String>,
    started: Vec<(String, String)>,
    ended: HashMap<String, Box<dyn FnOnce(ForkEnd) + Send>>,
    discarded: Vec<String>,
    /// The discards whose stop has not finished yet.
    stopping: Vec<Box<dyn FnOnce() + Send>>,
    reports: Vec<StatusReport>,
    log: Vec<String>,
    /// Whether the agent's input box holds text.
    draft: bool,
    /// The switches asked, by the session of their fork, and where each reports its end.
    switches: Vec<(String, SwitchDone)>,
}

type SwitchDone = Box<dyn FnOnce(Result<Agent, SwitchFailure>) + Send>;

impl FakeForks {
    fn host(&self) -> std::sync::MutexGuard<'_, FakeHost> {
        self.0.lock().unwrap()
    }

    /// The forks started, as (session, prompt), oldest first.
    fn started(&self) -> Vec<(String, String)> {
        self.host().started.clone()
    }

    /// The sessions of the forks discarded, in order.
    fn discarded(&self) -> Vec<String> {
        self.host().discarded.clone()
    }

    /// The agent's session moves: its last conversation entry is now `entry`.
    fn move_session(&self, entry: &str) {
        self.host().entry = Some(entry.into());
    }

    /// Herdr reports `status` for the watched agent.
    fn report(&self, status: AgentStatus) {
        let host = self.host();
        (host.reports.last().expect("the agent is watched"))(status);
    }

    /// The fork `session` ends, its turn done.
    fn end(&self, session: &str) {
        let ended = self.host().ended.remove(session).expect("the fork runs");
        ended(ForkEnd {
            exit: "exit status: 0".into(),
            usage: review_run_ahead::TokenUsage {
                input: 1,
                cache_creation: 2,
                cache_read: 3,
                output: 4,
            },
            finished: true,
        });
    }

    /// The discarded forks finish stopping.
    fn finish_stopping(&self) {
        let stopping = std::mem::take(&mut self.host().stopping);
        for done in stopping {
            done();
        }
    }

    fn log(&self) -> Vec<String> {
        self.host().log.clone()
    }

    /// The agent's input box holds text, or not.
    fn type_draft(&self, draft: bool) {
        self.host().draft = draft;
    }

    /// The sessions of the forks the agent was asked to switch to, in order.
    fn switches(&self) -> Vec<String> {
        self.host()
            .switches
            .iter()
            .map(|(session, _)| session.clone())
            .collect()
    }

    /// The latest switch ends with `result`.
    fn finish_switch(&self, result: Result<Agent, SwitchFailure>) {
        let (_, done) = self.host().switches.pop().expect("a switch runs");
        done(result);
    }
}

impl ForkHost for FakeForks {
    fn watch(&self, _pane: &PaneId, report: StatusReport) -> PaneWatch {
        self.host().reports.push(report);
        PaneWatch::new(())
    }

    fn point(&self, agent: &Agent) -> Result<ForkPoint, String> {
        Ok(ForkPoint {
            session: agent.agent_session.as_ref().unwrap().value.clone(),
            entry: self.host().entry.clone(),
            transcripts: "/fake/projects".into(),
            model: None,
            command: ForkCommand {
                program: "claude".into(),
                arguments: Vec::new(),
                directory: "/".into(),
                environment: Vec::new(),
            },
        })
    }

    fn last_entry(&self, _point: &ForkPoint) -> Option<String> {
        self.host().entry.clone()
    }

    fn start(
        &self,
        start: ForkStart<'_>,
        ended: Box<dyn FnOnce(ForkEnd) + Send>,
    ) -> Result<ProcessStamp, String> {
        let mut host = self.host();
        host.started
            .push((start.session.to_owned(), start.prompt.clone()));
        host.ended.insert(start.session.to_owned(), ended);
        let pid = 100_000 + u32::try_from(host.started.len()).unwrap();
        Ok(ProcessStamp { pid, started: 1 })
    }

    fn discard(&self, fork: ForkTrace<'_>, done: Box<dyn FnOnce() + Send>) {
        let mut host = self.host();
        host.discarded.push(fork.session.to_owned());
        host.stopping.push(done);
    }

    fn input_is_empty(&self, _pane: &PaneId) -> Result<bool, String> {
        Ok(!self.host().draft)
    }

    fn switch(&self, switch: SwitchTo<'_>, done: SwitchDone) {
        self.host()
            .switches
            .push((switch.fork.session.to_owned(), done));
    }

    fn log(&self, line: &str) {
        self.host().log.push(line.to_owned());
    }
}

impl Harness {
    /// Starts a round with run-ahead as `run_ahead`, whose agent asks its first question.
    fn ask(&mut self, run_ahead: RunAhead) -> (TurnRequest, String) {
        self.store.save_explore_run_ahead(run_ahead).unwrap();
        self.capture();
        let first = self.request(None);
        let access = self.turn(&first);
        assert!(applied(self.submit(&access, question(&first, 1))));
        (first, access)
    }

    /// Hands the session every input its threads left.
    fn pump(&mut self) {
        while let Ok(input) = self.inbox.try_recv() {
            self.session.handle(input);
        }
    }

    /// The forks saved beside the round.
    fn forks_saved(&self) -> RoundForks {
        self.store
            .load_round_forks(&self.unit, &self.exploration().instance)
            .unwrap()
    }

    /// Submits `operation` as the fork `session`, with the access its prompt granted.
    fn submit_as_fork(&mut self, session: &str, operation: Operation) -> Result<Response, String> {
        let access = fork_line(&self.fork_prompt(session), "Explore review access: ");
        self.submit(&access, operation)
    }

    fn fork_prompt(&self, session: &str) -> String {
        self.forks
            .started()
            .into_iter()
            .find(|(started, _)| started == session)
            .unwrap()
            .1
    }

    /// The turn the fork `session` was told, as its prompt names it.
    fn fork_request(&self, session: &str) -> TurnRequest {
        let prompt = self.fork_prompt(session);
        let mut exploration = self.exploration().clone();
        let question = exploration.questions.last().cloned();
        let choice = fork_line(&prompt, "Selected option ID: ");
        let mut request = exploration
            .request(
                Some(AnswerInput {
                    option: Some(choice),
                    ..AnswerInput::default()
                }),
                question.as_ref(),
            )
            .unwrap();
        request.request = fork_line(&prompt, "Explore request: ");
        request.answer.as_mut().unwrap().id = fork_line(&prompt, "Answer ID: ");
        request
    }
}

fn fork_line(prompt: &str, label: &str) -> String {
    prompt
        .lines()
        .find_map(|line| line.strip_prefix(label))
        .unwrap_or_else(|| panic!("no {label:?} in the prompt"))
        .to_owned()
}

fn discarded_for(record: &ForkRecord) -> Option<DiscardReason> {
    record.discarded.as_ref().map(|discard| discard.reason)
}

#[test]
fn every_choice_gets_a_fork_told_that_answer_with_its_own_request_answer_and_access() {
    let mut harness = Harness::start();

    let (_, access) = harness.ask(RunAhead::Every);

    let started = harness.forks.started();
    let saved = harness.forks_saved();
    let choices: Vec<_> = saved
        .forks
        .iter()
        .map(|fork| fork.choice.as_str())
        .collect();
    assert_eq!(choices, ["keep", "change"]);
    let instance = harness.exploration().instance.clone();
    let mut identities = std::collections::HashSet::new();
    for ((session, prompt), record) in started.iter().zip(&saved.forks) {
        assert_eq!(&record.session, session);
        assert_eq!(record.question, "q1");
        assert_eq!(record.process.unwrap().started, 1);
        assert_eq!(fork_line(prompt, "Explore round: "), instance);
        assert_eq!(fork_line(prompt, "Selected option ID: "), record.choice);
        assert_eq!(fork_line(prompt, "Question: "), "q1 (version 1)");
        assert_eq!(fork_line(prompt, "Shown as: "), "Q1");
        let fork_access = fork_line(prompt, "Explore review access: ");
        assert_ne!(fork_access, access, "a fork never holds the agent's access");
        assert!(identities.insert(fork_access));
        assert!(identities.insert(fork_line(prompt, "Explore request: ")));
        assert!(identities.insert(fork_line(prompt, "Answer ID: ")));
    }
    assert_eq!(
        harness.agents.prompts().len(),
        1,
        "the agent in the pane got only the kickoff"
    );
}

#[test]
fn with_run_ahead_off_no_fork_starts_and_nothing_is_saved_for_forks() {
    let mut harness = Harness::start();

    harness.ask(RunAhead::Off);

    assert!(harness.forks.started().is_empty());
    assert!(
        harness.forks.host().reports.is_empty(),
        "nothing watches the agent"
    );
    assert_eq!(harness.forks_saved(), RoundForks::default());
}

#[test]
fn the_recommended_setting_forks_only_the_choice_the_agent_recommends() {
    let mut harness = Harness::start();
    harness
        .store
        .save_explore_run_ahead(RunAhead::Recommended)
        .unwrap();
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    let Operation::SubmitQuestion(mut update) = question(&first, 1) else {
        unreachable!("a question");
    };
    update.next.as_mut().unwrap().alternatives[1].recommendation = Some("Safer.".into());

    assert!(applied(
        harness.submit(&access, Operation::SubmitQuestion(update))
    ));

    let saved = harness.forks_saved();
    let choices: Vec<_> = saved
        .forks
        .iter()
        .map(|fork| fork.choice.as_str())
        .collect();
    assert_eq!(choices, ["change"]);
}

#[test]
fn a_fork_s_turn_is_kept_for_its_choice_and_shown_to_nobody() {
    let mut harness = Harness::start();
    harness.ask(RunAhead::Every);
    let session = harness.forks.started()[0].0.clone();
    let request = harness.fork_request(&session);
    let before = harness.saved();
    while harness.events.try_recv().is_ok() {}

    let kept = harness.submit_as_fork(&session, question(&request, 2));

    assert_eq!(shown(kept), (true, Some("Q2".into())));
    assert_eq!(harness.saved(), before, "the round does not change");
    while let Ok(event) = harness.events.try_recv() {
        assert!(
            event
                .downcast_ref::<ui_events::ExploreCommitted>()
                .is_none(),
            "the pane hears nothing of the fork's turn"
        );
    }
    let record = &harness.forks_saved().forks[0];
    assert_eq!(record.turn.as_ref().unwrap().request, request.request);
    assert!(harness.forks_saved().forks[1].turn.is_none());
    assert_eq!(
        shown(harness.submit_as_fork(&session, question(&request, 2))),
        (false, None),
        "the same turn again changes nothing"
    );
    assert!(
        harness
            .submit_as_fork(&session, conclusion(&request, CONCLUSION))
            .is_err(),
        "another turn is refused"
    );
    assert_eq!(
        harness.forks_saved().forks[0]
            .turn
            .as_ref()
            .unwrap()
            .conclusion,
        None,
        "the first turn stays kept"
    );
}

#[test]
fn a_fork_may_submit_only_for_the_request_it_was_forked_for() {
    let mut harness = Harness::start();
    harness.ask(RunAhead::Every);
    let started = harness.forks.started();
    let other = harness.fork_request(&started[1].0);

    let own = harness.fork_request(&started[0].0);
    let refused = harness
        .submit_as_fork(&started[0].0, question(&other, 2))
        .unwrap_err();

    assert!(
        refused.contains(&own.request),
        "the refusal names the fork's own request"
    );
    assert!(
        harness
            .forks_saved()
            .forks
            .iter()
            .all(|fork| fork.turn.is_none())
    );
}

#[test]
fn an_answer_discards_every_fork_and_their_calls_are_refused_from_then_on() {
    let mut harness = Harness::start();
    harness.ask(RunAhead::Every);
    let started = harness.forks.started();
    let request = harness.fork_request(&started[0].0);

    harness.answer("");

    let sessions: Vec<_> = started.iter().map(|(session, _)| session.clone()).collect();
    assert_eq!(harness.forks.discarded(), sessions);
    harness.forks.finish_stopping();
    harness.pump();
    for record in &harness.forks_saved().forks {
        assert_eq!(discarded_for(record), Some(DiscardReason::Answered));
        assert!(record.cleaned);
    }
    assert!(
        harness
            .submit_as_fork(&started[0].0, question(&request, 2))
            .is_err()
    );
}

#[test]
fn a_reset_discards_the_forks() {
    let mut harness = Harness::start();
    harness.ask(RunAhead::Every);

    harness.session.handle(Input::Command(Command::Reset));

    assert_eq!(harness.forks.discarded().len(), 2);
    harness.forks.finish_stopping();
    harness.pump();
    let saved = harness.forks_saved();
    assert!(
        saved
            .forks
            .iter()
            .all(|fork| fork.cleaned && discarded_for(fork) == Some(DiscardReason::Reset))
    );
}

#[test]
fn cancelling_the_answer_a_question_followed_discards_its_forks_and_forks_the_question_again() {
    let mut harness = Harness::start();
    harness.ask(RunAhead::Every);
    let (answer, access) = harness.answer("");
    assert!(applied(harness.submit(&access, question(&answer, 2))));
    let second: Vec<_> = harness.forks.started()[2..].to_vec();
    assert_eq!(second.len(), 2);

    harness.session.handle(Input::Command(Command::CancelAnswer(
        answer.answer.unwrap().id,
    )));

    let discarded = harness.forks.discarded();
    assert_eq!(&discarded[2..], [second[0].0.clone(), second[1].0.clone()]);
    let saved = harness.forks_saved();
    assert!(
        saved.forks[2..4]
            .iter()
            .all(|fork| discarded_for(fork) == Some(DiscardReason::AnswerCancelled))
    );
    let again = harness.forks.started();
    assert_eq!(
        again.len(),
        6,
        "the first question waits again, with new forks"
    );
    assert_eq!(fork_line(&again[4].1, "Question: "), "q1 (version 1)");
}

#[test]
fn forks_are_taken_again_when_the_agent_worked_and_its_session_moved() {
    let mut harness = Harness::start();
    harness.ask(RunAhead::Every);
    let first: Vec<_> = harness.forks.started();

    // Work that leaves the session where it was takes nothing again.
    harness.forks.report(AgentStatus::Working);
    harness.forks.report(AgentStatus::Idle);
    harness.pump();
    assert_eq!(harness.forks.started().len(), 2);

    harness.forks.report(AgentStatus::Working);
    harness.forks.move_session("after-a-talk");
    harness.forks.report(AgentStatus::Idle);
    harness.pump();

    assert_eq!(
        harness.forks.discarded(),
        [first[0].0.clone(), first[1].0.clone()]
    );
    let saved = harness.forks_saved();
    assert_eq!(saved.forks.len(), 4);
    assert!(
        saved.forks[..2]
            .iter()
            .all(|fork| discarded_for(fork) == Some(DiscardReason::SessionMoved))
    );
    assert!(saved.forks[2..].iter().all(|fork| fork.discarded.is_none()));
}

#[test]
fn a_fork_s_end_and_tokens_are_saved() {
    let mut harness = Harness::start();
    harness.ask(RunAhead::Every);
    let session = harness.forks.started()[0].0.clone();

    harness.forks.end(&session);
    harness.pump();

    let record = &harness.forks_saved().forks[0];
    assert_eq!(record.exit.as_deref(), Some("exit status: 0"));
    assert_eq!(record.usage.unwrap().output, 4);
    assert!(
        harness
            .forks
            .log()
            .iter()
            .any(|line| line.contains(&session))
    );
}

#[test]
fn a_closing_reviewer_discards_its_forks() {
    let mut harness = Harness::start();
    harness.ask(RunAhead::Every);
    let forks = harness.forks.clone();
    // The fake stops at once: the close waits for every stop.
    let stopper = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        while forks.discarded().len() < 2 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        forks.finish_stopping();
    });

    harness.reopen();
    stopper.join().unwrap();

    let saved = harness.forks_saved();
    assert!(
        saved.forks[..2]
            .iter()
            .all(|fork| fork.cleaned && discarded_for(fork) == Some(DiscardReason::ReviewerClosed))
    );
}

#[test]
fn a_reopened_reviewer_discards_the_forks_a_stopped_reviewer_left_and_not_a_live_one_s() {
    let mut harness = Harness::start();
    harness.ask(RunAhead::Every);
    let unit = harness.unit.clone();
    let instance = harness.exploration().instance.clone();
    harness.forks.finish_stopping();
    // Two forks as reviewers left them that never discarded them: one stopped, one runs.
    let gone = ProcessStamp {
        pid: u32::MAX - 1,
        started: 1,
    };
    let running = ProcessStamp::of(std::os::unix::process::parent_id()).unwrap();
    harness
        .store
        .lock_explore(&unit)
        .unwrap()
        .update_round_forks(&instance, |forks| {
            for (fork, reviewer) in forks.forks.iter_mut().zip([gone, running]) {
                fork.reviewer = reviewer;
                fork.session = format!("left-by-{}", reviewer.pid);
            }
        })
        .unwrap();
    let forks = harness.forks.clone();
    let stopper = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        while forks.host().stopping.is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        forks.finish_stopping();
    });

    harness.reopen();
    stopper.join().unwrap();
    harness.forks.finish_stopping();
    harness.pump();

    let saved = harness.forks_saved();
    let discarded = harness.forks.discarded();
    assert!(discarded.contains(&saved.forks[0].session));
    assert!(!discarded.contains(&saved.forks[1].session));
    assert_eq!(
        discarded_for(&saved.forks[0]),
        Some(DiscardReason::ReviewerStopped)
    );
    assert!(saved.forks[0].cleaned);
    assert_eq!(saved.forks[1].discarded, None);
}

fn shown(result: Result<Response, String>) -> (bool, Option<String>) {
    match result {
        Ok(Response::Explore { applied, shown_as }) => {
            (applied, shown_as.map(|number| number.to_string()))
        }
        other => panic!("unexpected Explore response: {other:?}"),
    }
}

#[test]
fn a_fork_s_prompt_lists_the_unreviewed_lines_as_the_answer_leaves_them_and_marks_nothing() {
    let mut harness = Harness::start();
    harness.ask(RunAhead::Every);
    let (answer, access) = harness.answer("");
    let Operation::SubmitQuestion(mut update) = question(&answer, 2) else {
        unreachable!("a question");
    };
    // Once answered, the second question marks the whole change reviewed.
    update.reviewed = vec![
        serde_json::from_value(
            serde_json::json!({"path": "reviewed.rs", "side": "new", "lines": null}),
        )
        .unwrap(),
    ];

    assert!(applied(
        harness.submit(&access, Operation::SubmitQuestion(update))
    ));

    let (_, prompt) = harness.forks.started().remove(2);
    assert_eq!(fork_line(&prompt, "Question: "), "q2 (version 1)");
    assert!(
        !prompt.contains("herdr-review-unreviewed-"),
        "the prompt names no diffs directory: {}",
        fork_line(&prompt, "Unreviewed diffs: ")
    );
    let snapshot = complete_repository_snapshot(&harness.repository);
    let tracker =
        review_state::ReviewTracker::new(harness.repository.clone(), harness.store.clone());
    assert!(
        tracker
            .statuses(&snapshot)
            .unwrap()
            .iter()
            .all(|state| state.status != review_state::ReviewStatus::Reviewed),
        "the reviewer's marks do not change before the answer"
    );
}

#[test]
fn turning_run_ahead_off_discards_the_forks_at_the_next_input() {
    let mut harness = Harness::start();
    harness.ask(RunAhead::Every);

    harness.store.save_explore_run_ahead(RunAhead::Off).unwrap();
    harness.session.handle(Input::StorageChanged);

    assert_eq!(harness.forks.discarded().len(), 2);
    assert!(
        harness
            .forks_saved()
            .forks
            .iter()
            .all(|fork| { discarded_for(fork) == Some(DiscardReason::TurnedOff) })
    );
}

impl Harness {
    /// Answers the latest question with `choice` and the comment `text`, as the reviewer, and
    /// returns the saved turn; no prompt is awaited.
    fn post_answer(&mut self, choice: Option<&str>, text: &str) -> TurnRequest {
        let request = self.request(Some(AnswerInput {
            option: choice.map(str::to_owned),
            text: text.into(),
            ..AnswerInput::default()
        }));
        self.session
            .handle(Input::Command(Command::Turn(Box::new(request.clone()))));
        assert!(self.next::<ui_events::ExplorePosted>().result.is_ok());
        request
    }

    /// The fork for `choice`, by session, submits the next question for its turn.
    fn fork_submits(&mut self, choice: &str) -> (String, TurnRequest) {
        let saved = self.forks_saved();
        let session = saved
            .forks
            .iter()
            .rfind(|fork| fork.choice == choice)
            .unwrap()
            .session
            .clone();
        let request = self.fork_request(&session);
        assert!(applied(
            self.submit_as_fork(&session, question(&request, 2))
        ));
        (session, request)
    }

    /// Whether no prompt reached the agent in the pane beyond those the test read.
    fn no_prompt_sent(&self) -> bool {
        std::thread::sleep(Duration::from_millis(300));
        self.agents.prompts().len() == self.delivered
    }

    /// The agent in the pane, on the session `session`.
    fn agent_on(&self, session: &str) -> Agent {
        let mut agent = agent();
        agent.agent_session.as_mut().unwrap().value = session.to_owned();
        self.agents.upsert_agent(agent.clone());
        agent
    }

    /// The path the latest answer's turn took, as saved beside the round.
    fn latest_path(&self) -> TurnPath {
        self.forks_saved().answers.last().unwrap().path.clone()
    }
}

/// The turn the agent submits with `operation`.
fn turn_of(operation: &Operation) -> InterviewUpdate {
    crate::submission::update_of(operation).unwrap()
}

#[test]
fn a_bare_answer_whose_fork_submitted_switches_the_agent_to_it_and_takes_its_turn() {
    let mut harness = Harness::start();
    harness.ask(RunAhead::Every);
    let (fork, _) = harness.fork_submits("keep");
    let other = harness.forks_saved().forks[1].session.clone();
    let fork_access = fork_line(&harness.fork_prompt(&fork), "Explore review access: ");

    let request = harness.post_answer(Some("keep"), " ");

    assert_eq!(
        harness.forks.switches(),
        std::slice::from_ref(&fork),
        "{:?}",
        harness.forks.log()
    );
    assert_eq!(harness.forks.discarded(), [other], "the other fork goes");
    assert!(matches!(
        harness.page.stage(),
        RoundStage::AgentWorking { .. }
    ));
    let switched = harness.agent_on(&fork);
    harness.forks.finish_switch(Ok(switched.clone()));
    harness.pump();

    assert!(
        harness.no_prompt_sent(),
        "the agent in the pane gets no prompt"
    );
    let mut prepared = None;
    while let Ok(event) = harness.events.try_recv() {
        if let Some(event) = event.downcast_ref::<ui_events::ExploreTurnPrepared>() {
            prepared = Some(event.request.clone());
        }
    }
    assert_eq!(
        prepared.as_ref(),
        Some(&request.request),
        "the pane hears that the turn was prepared"
    );
    let saved = harness.saved();
    assert_eq!(
        saved.exploration.conversation.last().unwrap().update,
        turn_of(&question(&request, 2)),
        "the round has the turn the agent would have submitted for the answer"
    );
    assert_eq!(
        saved.turns[&request.request].state,
        review_explore::DispatchState::Delivered
    );
    assert_eq!(
        saved.last_agent_session,
        review_explore::ConversationBinding::from_agent(&switched)
    );
    let forks = harness.forks_saved();
    assert_eq!(
        harness.latest_path(),
        TurnPath::Prepared {
            session: fork.clone()
        }
    );
    assert!(matches!(
        forks.forks[0].continued,
        Some(Continuation::Switched { .. })
    ));
    assert!(!forks.forks[0].cleaned && forks.forks[0].discarded.is_none());
    let RoundStage::Question { response, .. } = harness.page.stage() else {
        panic!("the page shows the next question");
    };
    assert!(response.prepared, "the page says the turn was prepared");

    // The agent continues as the fork: its access holds until its next prompt, from the
    // fork's session only.
    assert!(
        !shown(harness.submit(&fork_access, question(&request, 2))).0,
        "the same turn again changes nothing"
    );
    harness.agent_on("conversation");
    assert!(
        harness.submit(&fork_access, question(&request, 2)).is_err(),
        "the session the agent left may no longer call"
    );
    harness.agent_on(&fork);
    harness.exploration = Some(saved.exploration);
    let (next, access) = harness.answer("");
    assert!(applied(harness.submit(&access, question(&next, 3))));
}

#[test]
fn an_answer_the_fork_was_not_told_exactly_goes_to_the_agent_and_the_reason_is_recorded() {
    type Setup = fn(&mut Harness) -> (Option<&'static str>, &'static str);
    let cases: [(PlainReason, Setup); 9] = [
        (PlainReason::Comment, |_| {
            (Some("keep"), "Keep it, with a test.")
        }),
        (PlainReason::NoneOfTheAbove, |_| {
            (Some("none-of-the-above"), "Neither: drop the policy.")
        }),
        (PlainReason::StillWorking, |_| (Some("change"), "")),
        (PlainReason::NoTurn, |harness| {
            let session = harness.forks_saved().forks[1].session.clone();
            harness.forks.end(&session);
            harness.pump();
            (Some("change"), "")
        }),
        (PlainReason::InputNotEmpty, |harness| {
            harness.forks.type_draft(true);
            (Some("keep"), "")
        }),
        (PlainReason::SessionMoved, |harness| {
            harness.forks.move_session("after-a-talk");
            (Some("keep"), "")
        }),
        (PlainReason::AgentBusy, |harness| {
            let mut busy = agent();
            busy.agent_status = AgentStatus::Working;
            harness.agents.upsert_agent(busy);
            (Some("keep"), "")
        }),
        (PlainReason::ChatMessage, |harness| {
            let instance = harness.exploration().instance.clone();
            harness
                .store
                .update_threads(&harness.unit, |threads| {
                    threads
                        .post(review_threads::Post::to_round(
                            &instance,
                            "Why does the policy exist?".into(),
                            Some(review_threads::AskedUnder::Question {
                                question: "q1".into(),
                                version: 1,
                                number: None,
                            }),
                            None,
                        ))
                        .map(|_| ())
                })
                .unwrap();
            (Some("keep"), "")
        }),
        (PlainReason::UnreviewedChanged, |harness| {
            let snapshot = complete_repository_snapshot(&harness.repository);
            let other = snapshot
                .files
                .iter()
                .find(|file| file.review_path().display() == "other.rs")
                .unwrap();
            review_state::ReviewTracker::new(harness.repository.clone(), harness.store.clone())
                .mark(&snapshot, other, &review_types::MarkAuthor::Reviewer)
                .unwrap();
            (Some("keep"), "")
        }),
    ];
    for (reason, setup) in cases {
        let mut harness = Harness::start();
        harness.files.write("other.rs", b"pub fn other() {}\n");
        harness.ask(RunAhead::Every);
        harness.fork_submits("keep");
        let (choice, text) = setup(&mut harness);

        let request = harness.post_answer(choice, text);

        assert_eq!(
            harness.latest_path(),
            TurnPath::Plain {
                reason: reason.clone()
            }
        );
        assert!(harness.forks.switches().is_empty(), "{reason:?}");
        assert_eq!(harness.forks.discarded().len(), 2, "{reason:?}");
        if reason != PlainReason::AgentBusy {
            let prompt = harness.delivered_prompt();
            assert!(
                prompt.contains(&format!("Explore request: {}\n", request.request)),
                "{reason:?}: the agent in the pane gets the answer"
            );
        }
    }
}

#[test]
fn a_switch_that_fails_leaves_the_answer_for_retry_which_goes_to_the_agent_in_the_pane() {
    let mut harness = Harness::start();
    harness.ask(RunAhead::Every);
    let (fork, _) = harness.fork_submits("keep");
    let request = harness.post_answer(Some("keep"), "");

    harness.forks.finish_switch(Err(SwitchFailure {
        error: "the agent in the pane is working".into(),
        typed: false,
    }));
    harness.pump();

    let finished = harness.next::<ui_events::ExploreFinished>();
    assert!(finished.result.unwrap_err().contains("is working"));
    assert!(matches!(
        harness.page.stage(),
        RoundStage::Interrupted { .. }
    ));
    assert_eq!(
        harness.latest_path(),
        TurnPath::Plain {
            reason: PlainReason::SwitchFailed {
                error: "the agent in the pane is working".into()
            }
        }
    );
    assert!(harness.forks.discarded().contains(&fork));
    assert!(harness.saved().exploration.conversation.len() == 1);

    harness
        .session
        .handle(Input::Command(Command::Retry(Box::new(request.clone()))));

    assert!(
        harness
            .delivered_prompt()
            .contains(&format!("Explore request: {}\n", request.request))
    );
}

#[test]
fn a_reopened_reviewer_shows_the_prepared_turn_and_keeps_the_session_the_agent_runs() {
    let mut harness = Harness::start();
    harness.ask(RunAhead::Every);
    let (fork, _) = harness.fork_submits("keep");
    let request = harness.post_answer(Some("keep"), "");
    let switched = harness.agent_on(&fork);
    harness.forks.finish_switch(Ok(switched));
    harness.pump();
    let instance = harness.exploration().instance.clone();
    let unit = harness.unit.clone();
    // As a reviewer that stopped left it.
    harness
        .store
        .lock_explore(&unit)
        .unwrap()
        .update_round_forks(&instance, |forks| {
            forks.forks[0].reviewer = ProcessStamp {
                pid: u32::MAX - 1,
                started: 1,
            };
        })
        .unwrap();
    harness.forks.finish_stopping();
    harness.pump();
    let forks = harness.forks.clone();
    // The closing reviewer waits for the forks of the next question to stop.
    let stopper = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        while forks.host().stopping.is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        forks.finish_stopping();
    });

    let restored = harness.reopen();
    stopper.join().unwrap();

    assert_eq!(restored.prepared_turns, [request.request]);
    assert!(!harness.forks.discarded().contains(&fork));
    let RoundStage::Question { response, .. } = harness.page.stage() else {
        panic!("the page shows the next question");
    };
    assert!(response.prepared);
}

#[test]
fn a_turn_saved_while_the_agent_switches_waits_and_goes_to_the_fork_s_session() {
    let mut harness = Harness::start();
    harness.ask(RunAhead::Every);
    let (fork, _) = harness.fork_submits("keep");
    let switching = harness.post_answer(Some("keep"), "");
    harness.session.handle(Input::Command(Command::CancelAnswer(
        switching.answer.unwrap().id,
    )));
    let cancelled = harness.next::<ui_events::ExploreAnswerCancelled>();
    harness.exploration = Some(cancelled.result.unwrap().exploration.clone());

    let again = harness.post_answer(Some("keep"), "Keep it, with a test.");

    assert!(
        harness.no_prompt_sent(),
        "no prompt while the agent switches"
    );
    let switched = harness.agent_on(&fork);
    harness.forks.finish_switch(Ok(switched.clone()));
    harness.pump();
    let prompt = harness.delivered_prompt();
    assert!(prompt.contains(&format!("Explore request: {}\n", again.request)));
    let saved = harness.saved();
    assert_eq!(
        saved.last_agent_session,
        review_explore::ConversationBinding::from_agent(&switched),
        "the round follows the session the agent runs"
    );
    let answers = harness.forks_saved().answers;
    assert_eq!(
        answers[0].path,
        TurnPath::Plain {
            reason: PlainReason::Withdrawn
        }
    );
}
