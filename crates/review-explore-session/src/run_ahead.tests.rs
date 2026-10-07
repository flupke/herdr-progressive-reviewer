//! Run-ahead in the session, with a fake agent host: which forks start, what their prompts
//! carry, what a fork may submit, and when forks are discarded.

use std::collections::HashMap;
use std::sync::{Condvar, Mutex};

use agent_fork::{ForkCommand, ProcessStamp};
use review_explore_page::RoundStage;
use review_explore_round_settings::RunAhead;
use review_run_ahead::{
    Continuation, DiscardReason, ForkEnd, ForkHost, ForkPoint, ForkRecord, ForkStart, ForkTrace,
    PaneWatch, PlainReason, RoundForks, StatusReport, SwitchFailure, SwitchTo, TurnPath,
};

use super::*;

/// An agent host that starts no process: it records the forks it is asked to start and
/// discard, and lets a test end them, report the agent's status and move its session. A fork
/// it discards stops at once, unless the test holds the stops.
#[derive(Clone, Default)]
pub(super) struct FakeForks {
    host: Arc<Mutex<FakeHost>>,
    /// Signalled each time the agent is asked to switch or to resume a session, and each time
    /// a stop is held.
    changed: Arc<Condvar>,
}

#[derive(Default)]
struct FakeHost {
    /// The last conversation entry of the agent's session, as it is now.
    entry: Option<String>,
    started: Vec<(String, String)>,
    ended: HashMap<String, Box<dyn FnOnce(ForkEnd) + Send>>,
    discarded: Vec<String>,
    /// Whether a discarded fork keeps running until the test lets it stop.
    hold_stops: bool,
    /// The stops held, and where each reports that its fork stopped.
    stopping: Vec<Box<dyn FnOnce() + Send>>,
    reports: Vec<StatusReport>,
    log: Vec<String>,
    /// The switches asked, by the session of their fork, and where each reports its end.
    switches: Vec<(String, SwitchDone)>,
    /// The sessions the agent was asked to resume, and where each reports its end.
    resumes: Vec<(String, SwitchDone)>,
}

type SwitchDone = Box<dyn FnOnce(Result<Agent, SwitchFailure>) + Send>;

/// The sessions of `moves`, in order.
fn sessions(moves: &[(String, SwitchDone)]) -> Vec<String> {
    moves.iter().map(|(session, _)| session.clone()).collect()
}

impl FakeForks {
    fn host(&self) -> std::sync::MutexGuard<'_, FakeHost> {
        self.host.lock().unwrap()
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

    /// The fork `session` ends without its turn done, as `exit` says.
    fn end_as(&self, session: &str, exit: &str, finished: bool) {
        let ended = self.host().ended.remove(session).expect("the fork runs");
        ended(ForkEnd {
            exit: exit.into(),
            usage: review_run_ahead::TokenUsage::default(),
            finished,
        });
    }

    fn log(&self) -> Vec<String> {
        self.host().log.clone()
    }

    /// Whether the forks discarded from now on keep running until the test lets them stop.
    fn hold_stops(&self, hold: bool) {
        self.host().hold_stops = hold;
    }

    /// Waits until `count` stops are held, then lets those forks stop.
    fn finish_stopping(&self, count: usize) {
        let stopping = std::mem::take(
            &mut self
                .changed
                .wait_while(self.host(), |host| host.stopping.len() < count)
                .unwrap()
                .stopping,
        );
        for done in stopping {
            done();
        }
    }

    /// Waits until `moves` of the host holds a move: the session starts each move on a thread
    /// of its own. Gives up after ten seconds, for a test that fails.
    fn wait_for(&self, moves: fn(&mut FakeHost) -> &mut Vec<(String, SwitchDone)>) {
        drop(
            self.changed
                .wait_timeout_while(self.host(), Duration::from_secs(10), |host| {
                    moves(host).is_empty()
                })
                .unwrap(),
        );
    }

    /// The sessions of the forks the agent was asked to switch to, in order, once it was asked
    /// to switch.
    fn switches(&self) -> Vec<String> {
        self.wait_for(|host| &mut host.switches);
        sessions(&self.host().switches)
    }

    /// The latest switch ends with `result`.
    fn finish_switch(&self, result: Result<Agent, SwitchFailure>) {
        self.wait_for(|host| &mut host.switches);
        let (_, done) = self.host().switches.pop().expect("a switch runs");
        done(result);
    }

    /// The sessions the agent was asked to resume, outside a switch, in order, once it was
    /// asked to resume one.
    fn resumes(&self) -> Vec<String> {
        self.wait_for(|host| &mut host.resumes);
        sessions(&self.host().resumes)
    }

    /// The latest resume ends with `result`.
    fn finish_resume(&self, result: Result<Agent, SwitchFailure>) {
        self.wait_for(|host| &mut host.resumes);
        let (_, done) = self.host().resumes.pop().expect("a resume runs");
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
        if host.hold_stops {
            host.stopping.push(done);
            drop(host);
            self.changed.notify_all();
        } else {
            drop(host);
            done();
        }
    }

    fn switch(&self, switch: SwitchTo<'_>, done: SwitchDone) {
        self.host()
            .switches
            .push((switch.fork.session.to_owned(), done));
        self.changed.notify_all();
    }

    fn resume(&self, _pane: &PaneId, session: &str, done: SwitchDone) {
        self.host().resumes.push((session.to_owned(), done));
        self.changed.notify_all();
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
    // Each choice's fork is told the same request and answer as before.
    let identities = |prompt: &str| {
        (
            fork_line(prompt, "Selected option ID: "),
            fork_line(prompt, "Explore request: "),
            fork_line(prompt, "Answer ID: "),
        )
    };
    let started = harness.forks.started();
    let before: Vec<_> = started[..2]
        .iter()
        .map(|(_, prompt)| identities(prompt))
        .collect();
    let again: Vec<_> = started[2..]
        .iter()
        .map(|(_, prompt)| identities(prompt))
        .collect();
    assert_eq!(again, before);
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
fn a_closing_reviewer_discards_its_forks_and_waits_for_them_to_stop() {
    let mut harness = Harness::start();
    harness.ask(RunAhead::Every);
    harness.forks.hold_stops(true);
    let forks = harness.forks.clone();
    // The forks stop once the closing reviewer asked both to.
    let stopper = std::thread::spawn(move || forks.finish_stopping(2));

    harness.reopen();
    stopper.join().unwrap();
    harness.forks.hold_stops(false);

    let saved = harness.forks_saved();
    assert!(
        saved.forks[..2]
            .iter()
            .all(|fork| fork.cleaned && discarded_for(fork) == Some(DiscardReason::ReviewerClosed))
    );
}

#[test]
fn a_closing_reviewer_gives_up_on_forks_that_do_not_stop_in_time() {
    let mut harness = Harness::start();
    harness.ask(RunAhead::Every);
    harness.forks.hold_stops(true);
    harness.session.wait_on_close(Duration::ZERO);

    harness.reopen();
    harness.forks.hold_stops(false);

    let saved = harness.forks_saved();
    assert_eq!(harness.forks.discarded().len(), 2);
    assert!(
        saved.forks[..2]
            .iter()
            .all(|fork| !fork.cleaned && discarded_for(fork) == Some(DiscardReason::ReviewerClosed)),
        "the forks are discarded, though not known to be stopped"
    );
}

#[test]
fn a_reopened_reviewer_discards_the_forks_a_stopped_reviewer_left_and_not_a_live_one_s() {
    let mut harness = Harness::start();
    harness.ask(RunAhead::Every);
    let unit = harness.unit.clone();
    let instance = harness.exploration().instance.clone();
    // Two forks as reviewers left them that never discarded them: one stopped, one runs.
    let gone = ProcessStamp {
        pid: u32::MAX - 1,
        started: 1,
    };
    let live = RunningProcess::start();
    let running = live.stamp();
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

    harness.reopen();
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

/// A process other than the test's own that runs until it is dropped, as another reviewer that
/// still runs.
struct RunningProcess(std::process::Child);

impl RunningProcess {
    /// Starts a process that waits for input the test never writes.
    fn start() -> Self {
        Self(
            std::process::Command::new("cat")
                .stdin(std::process::Stdio::piped())
                .spawn()
                .unwrap(),
        )
    }

    fn stamp(&self) -> ProcessStamp {
        ProcessStamp::of(self.0.id()).unwrap()
    }
}

impl Drop for RunningProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
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
        self.post_turn(&request)
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

    /// Whether no prompt reached the agent in the pane beyond those the test read, once every
    /// prompt sent so far went out or is held.
    fn no_prompt_sent(&self) -> bool {
        self.threads.prompt_sender().flush();
        self.agents.prompts().len() == self.delivered
    }

    /// Whether the agent was asked neither to switch nor to resume a session, and the session
    /// does not move it.
    fn nothing_moved(&self) -> bool {
        let host = self.forks.host();
        !self.session.moves_agent() && host.switches.is_empty() && host.resumes.is_empty()
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

    /// The paths of turns the pane heard of since the test last read the session's events, by
    /// request, oldest first.
    fn paths_heard(&self) -> Vec<(String, TurnPath)> {
        let mut heard = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            if let Some(event) = event.downcast_ref::<ui_events::ExploreTurnPath>() {
                heard.push((event.request.clone(), event.path.clone()));
            }
        }
        heard
    }

    /// The agent in the pane takes the turn `request` that the plain chain sent it, and posts
    /// the next question; returns the path the page shows with it.
    fn agent_takes(&mut self, request: &TurnRequest) -> Option<TurnPath> {
        let prompt = self.delivered_prompt();
        assert!(prompt.contains(&format!("Explore request: {}\n", request.request)));
        let access = fork_line(&prompt, "Explore review access: ");
        let Operation::SubmitQuestion(mut update) = question(request, 2) else {
            unreachable!("a question");
        };
        let changes = request
            .answer
            .as_ref()
            .and_then(|answer| answer.option.as_ref())
            .is_some_and(|choice| choice.id == "change");
        if let (true, Some(interpretation)) = (changes, &mut update.interpretation) {
            interpretation.status = review_explore::TopicStatus::NeedsFollowUp;
        }
        assert!(applied(
            self.submit(&access, Operation::SubmitQuestion(update))
        ));
        let RoundStage::Question { response, .. } = self.page.stage() else {
            panic!("the page shows the next question");
        };
        response.path
    }

    /// The reviewer writes `text` in the round's conversation, under the first question;
    /// returns the conversation and the message.
    fn write_in_chat(&self, text: &str) -> (review_threads::ThreadId, review_threads::MessageId) {
        let instance = self.exploration().instance.clone();
        let post = review_threads::Post::to_round(
            &instance,
            text.into(),
            Some(review_threads::AskedUnder::Question {
                question: "q1".into(),
                version: 1,
                number: None,
            }),
            None,
        );
        let written = (post.thread_id().clone(), post.message().id.clone());
        self.store
            .update_threads(&self.unit, |threads| threads.post(post).map(|_| ()))
            .unwrap();
        written
    }
}

impl Harness {
    /// The reviewer writes `text` in the round's conversation, under the first question, and the
    /// session hears of it as the thread service tells it; returns the conversation and the
    /// message.
    fn say_in_chat(&mut self, text: &str) -> (review_threads::ThreadId, review_threads::MessageId) {
        let (conversation, message) = self.write_in_chat(text);
        self.session
            .handle(Input::RoundMessage(review_thread_service::RoundMessage {
                review_unit: self.unit.clone(),
                round: self.exploration().instance.clone(),
                message: message.clone(),
            }));
        (conversation, message)
    }

    /// The agent replies to `message` in `conversation`.
    fn reply_in_chat(
        &self,
        conversation: &review_threads::ThreadId,
        message: &review_threads::MessageId,
    ) {
        self.store
            .update_threads(&self.unit, |threads| {
                threads
                    .post(review_threads::Post::answer(
                        conversation.clone(),
                        review_threads::MessageId::parse(&uuid::Uuid::new_v4().to_string())
                            .unwrap(),
                        "It keeps old drafts.".into(),
                        message.clone(),
                    ))
                    .map(|_| ())
            })
            .unwrap();
    }

    /// The agent takes the turn the reviewer's `message` in `conversation` woke it for: it
    /// works, replies, its session moves to `entry`, and it is idle again.
    fn agent_replies(
        &mut self,
        conversation: &review_threads::ThreadId,
        message: &review_threads::MessageId,
        entry: &str,
    ) {
        self.forks.report(AgentStatus::Working);
        self.reply_in_chat(conversation, message);
        self.forks.move_session(entry);
        self.forks.report(AgentStatus::Idle);
        self.pump();
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
    let prepared = TurnPath::Prepared {
        session: fork.clone(),
    };
    assert_eq!(
        harness.paths_heard(),
        [(request.request.clone(), prepared.clone())],
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
    assert_eq!(
        response.path,
        Some(prepared),
        "the page says the turn was prepared"
    );

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

/// The reviewer answers, after `setup`, as it returns: with the choice and the comment it
/// returns, which the fork for "keep" was not told exactly. The answer goes to the agent in the
/// pane, and the round records `reason`.
fn an_answer_the_fork_was_not_told_exactly_goes_to_the_agent_and_the_reason_is_recorded(
    reason: &PlainReason,
    setup: impl FnOnce(&mut Harness) -> (Option<&'static str>, &'static str),
) {
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
    assert!(harness.nothing_moved(), "{reason:?}");
    assert_eq!(harness.forks.discarded().len(), 2, "{reason:?}");
    let path = TurnPath::Plain {
        reason: reason.clone(),
    };
    assert!(
        harness
            .paths_heard()
            .contains(&(request.request.clone(), path.clone())),
        "{reason:?}: the pane hears why"
    );
    // The agent in the pane gets the answer, takes the turn, and the page says why with it.
    if *reason != PlainReason::AgentBusy {
        assert_eq!(harness.agent_takes(&request), Some(path), "{reason:?}");
    }
}

#[test]
fn an_answer_with_a_comment_goes_to_the_agent() {
    an_answer_the_fork_was_not_told_exactly_goes_to_the_agent_and_the_reason_is_recorded(
        &PlainReason::Comment,
        |_| (Some("keep"), "Keep it, with a test."),
    );
}

#[test]
fn an_answer_none_of_the_above_goes_to_the_agent() {
    an_answer_the_fork_was_not_told_exactly_goes_to_the_agent_and_the_reason_is_recorded(
        &PlainReason::NoneOfTheAbove,
        |_| (Some("none-of-the-above"), "Neither: drop the policy."),
    );
}

#[test]
fn an_answer_whose_fork_still_works_goes_to_the_agent() {
    an_answer_the_fork_was_not_told_exactly_goes_to_the_agent_and_the_reason_is_recorded(
        &PlainReason::StillWorking,
        |_| (Some("change"), ""),
    );
}

#[test]
fn an_answer_whose_fork_ended_without_a_turn_goes_to_the_agent() {
    an_answer_the_fork_was_not_told_exactly_goes_to_the_agent_and_the_reason_is_recorded(
        &PlainReason::NoTurn,
        |harness| {
            let session = harness.forks_saved().forks[1].session.clone();
            harness.forks.end(&session);
            harness.pump();
            (Some("change"), "")
        },
    );
}

#[test]
fn an_answer_after_the_agent_s_session_moved_goes_to_the_agent() {
    an_answer_the_fork_was_not_told_exactly_goes_to_the_agent_and_the_reason_is_recorded(
        &PlainReason::SessionMoved,
        |harness| {
            harness.forks.move_session("after-a-talk");
            (Some("keep"), "")
        },
    );
}

#[test]
fn an_answer_while_the_agent_is_busy_goes_to_the_agent() {
    an_answer_the_fork_was_not_told_exactly_goes_to_the_agent_and_the_reason_is_recorded(
        &PlainReason::AgentBusy,
        |harness| {
            let mut busy = agent();
            busy.agent_status = AgentStatus::Working;
            harness.agents.upsert_agent(busy);
            (Some("keep"), "")
        },
    );
}

#[test]
fn an_answer_after_a_chat_message_goes_to_the_agent() {
    an_answer_the_fork_was_not_told_exactly_goes_to_the_agent_and_the_reason_is_recorded(
        &PlainReason::ChatMessage,
        |harness| {
            harness.write_in_chat("Why does the policy exist?");
            (Some("keep"), "")
        },
    );
}

#[test]
fn an_answer_after_the_unreviewed_lines_changed_goes_to_the_agent() {
    an_answer_the_fork_was_not_told_exactly_goes_to_the_agent_and_the_reason_is_recorded(
        &PlainReason::UnreviewedChanged,
        |harness| {
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
        },
    );
}

#[test]
fn an_answer_whose_choice_has_no_fork_yet_says_why() {
    // The agent was still working when the question came.
    let mut harness = Harness::start();
    harness
        .store
        .save_explore_run_ahead(RunAhead::Every)
        .unwrap();
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    let mut busy = agent();
    busy.agent_status = AgentStatus::Working;
    harness.agents.upsert_agent(busy);
    assert!(applied(harness.submit(&access, question(&first, 1))));
    let request = harness.post_answer(Some("keep"), "");
    assert!(matches!(
        harness.latest_path(),
        TurnPath::Plain {
            reason: PlainReason::NoForks { .. }
        }
    ));
    assert_eq!(
        harness.paths_heard(),
        [(request.request, harness.latest_path())],
        "the pane hears why"
    );
}

#[test]
fn an_answer_whose_choice_the_setting_does_not_fork_says_why() {
    // The setting prepares the recommended choice only.
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
    harness.exploration = Some(harness.saved().exploration);
    let request = harness.post_answer(Some("keep"), "");
    let not_forked = TurnPath::Plain {
        reason: PlainReason::NotForked,
    };
    assert_eq!(harness.latest_path(), not_forked);
    assert_eq!(harness.agent_takes(&request), Some(not_forked));
}

#[test]
fn with_run_ahead_off_an_answer_says_nothing_of_forks() {
    let mut harness = Harness::start();
    harness.ask(RunAhead::Off);
    let request = harness.post_answer(Some("keep"), "Keep it, with a test.");
    assert_eq!(harness.agent_takes(&request), None);
    assert!(harness.paths_heard().is_empty());
    assert!(harness.forks_saved().answers.is_empty());
}

#[test]
fn a_message_in_the_chat_discards_every_fork_at_once_and_none_is_taken_before_the_agent_replied() {
    let mut harness = Harness::start();
    harness.ask(RunAhead::Every);
    let (submitted, _) = harness.fork_submits("keep");
    let first: Vec<_> = harness.forks.started();
    // A message in another round's conversation changes nothing here.
    harness
        .session
        .handle(Input::RoundMessage(review_thread_service::RoundMessage {
            review_unit: harness.unit.clone(),
            round: "another-round".into(),
            message: review_threads::MessageId::parse(&uuid::Uuid::new_v4().to_string()).unwrap(),
        }));
    assert!(harness.forks.discarded().is_empty());

    harness.say_in_chat("Why does the policy exist?");

    assert_eq!(
        harness.forks.discarded(),
        [first[0].0.clone(), first[1].0.clone()],
        "the fork that submitted goes too"
    );
    assert!(first.iter().any(|(session, _)| *session == submitted));
    assert!(
        harness
            .forks_saved()
            .forks
            .iter()
            .all(|fork| discarded_for(fork) == Some(DiscardReason::ChatMessage))
    );
    // The agent works on something else and its session moves, but it did not reply yet.
    harness.forks.report(AgentStatus::Working);
    harness.forks.move_session("before-the-reply");
    harness.forks.report(AgentStatus::Idle);
    harness.pump();
    assert_eq!(harness.forks.started().len(), first.len());
    assert!(!harness.session.quiet_wait_runs());

    harness.post_answer(Some("keep"), "");
    assert_eq!(
        harness.latest_path(),
        TurnPath::Plain {
            reason: PlainReason::ChatMessage
        }
    );
}

#[test]
fn a_talk_of_several_messages_takes_one_set_of_forks_once_it_was_quiet() {
    let mut harness = Harness::start();
    harness.ask(RunAhead::Every);
    let first = harness.forks.started().len();

    let (conversation, message) = harness.say_in_chat("Why does the policy exist?");
    harness.agent_replies(&conversation, &message, "first-reply");
    assert!(harness.session.quiet_wait_runs());
    // A new message before the wait passed starts it again, from the agent's next reply.
    let (_, message) = harness.say_in_chat("And for old drafts?");
    assert!(!harness.session.quiet_wait_runs());
    harness.agent_replies(&conversation, &message, "second-reply");
    assert!(harness.session.quiet_wait_runs());
    // Work of the agent stops the wait too; its next idle starts it again.
    harness.forks.report(AgentStatus::Working);
    harness.pump();
    assert!(!harness.session.quiet_wait_runs());
    harness.forks.report(AgentStatus::Idle);
    harness.pump();
    let (_, message) = harness.say_in_chat("Fine, thanks.");
    harness.agent_replies(&conversation, &message, "third-reply");
    assert_eq!(
        harness.forks.started().len(),
        first,
        "no fork during the talk"
    );

    harness.session.end_quiet_wait();

    assert_eq!(harness.forks.started().len(), first * 2);
    // Once taken, the forks stay while the agent's session stays where the talk left it.
    harness.forks.report(AgentStatus::Working);
    harness.forks.report(AgentStatus::Idle);
    harness.pump();
    assert_eq!(harness.forks.started().len(), first * 2);
    assert!(
        harness.forks_saved().forks[first..]
            .iter()
            .all(|fork| fork.discarded.is_none())
    );
}

#[test]
fn a_message_the_agent_has_not_replied_to_when_the_question_comes_holds_the_forks_until_it_did() {
    let mut harness = Harness::start();
    harness
        .store
        .save_explore_run_ahead(RunAhead::Every)
        .unwrap();
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    // The reviewer writes while the agent takes the turn that asks the question.
    let (conversation, message) = harness.say_in_chat("Why does the policy exist?");

    assert!(applied(harness.submit(&access, question(&first, 1))));

    assert!(harness.forks.started().is_empty());
    harness.agent_replies(&conversation, &message, "after-the-reply");
    assert!(harness.forks.started().is_empty());
    harness.session.end_quiet_wait();
    assert_eq!(harness.forks.started().len(), 2);
}

#[test]
fn a_message_posted_before_the_question_s_turn_does_not_hold_its_forks() {
    let mut harness = Harness::start();
    harness
        .store
        .save_explore_run_ahead(RunAhead::Every)
        .unwrap();
    harness.capture();
    let first = harness.request(None);
    // A message the agent never replied to, from before the turn that asks the question.
    harness.write_in_chat("Anything to add?");
    let before = review_explore::now_ms() - 1_000;
    harness
        .store
        .update_threads(&harness.unit, |threads| {
            threads.stamp_postings(|_| before);
            Ok(())
        })
        .unwrap();
    let access = harness.turn(&first);

    assert!(applied(harness.submit(&access, question(&first, 1))));

    assert_eq!(harness.forks.started().len(), 2);
}

#[test]
fn the_forks_are_taken_again_once_the_quiet_wait_passed_after_the_agent_s_reply() {
    let mut harness = Harness::start();
    harness.session.quiet_for(Duration::ZERO);
    harness.ask(RunAhead::Every);
    let first = harness.forks.started().len();
    let (conversation, message) = harness.say_in_chat("Why does the policy exist?");

    harness.agent_replies(&conversation, &message, "after-the-talk");

    // The quiet wait passes on a thread of its own, and tells the session through its inbox.
    while harness.forks.started().len() == first {
        let input = harness.inbox.recv().unwrap();
        harness.session.handle(input);
    }
    assert_eq!(harness.forks.started().len(), first * 2);
}

#[test]
fn after_a_talk_in_the_chat_forks_taken_again_hold_it_and_a_bare_answer_uses_one() {
    let mut harness = Harness::start();
    harness.ask(RunAhead::Every);
    let first = harness.forks.started();
    let (conversation, message) = harness.say_in_chat("Why does the policy exist?");
    harness.agent_replies(&conversation, &message, "after-the-talk");
    // The talk is older than the forks taken next: one taken in the same millisecond as a
    // message cannot be told apart from it.
    let before = review_explore::now_ms() - 1;
    harness
        .store
        .update_threads(&harness.unit, |threads| {
            threads.stamp_postings(|_| before);
            Ok(())
        })
        .unwrap();

    harness.session.end_quiet_wait();

    assert_eq!(harness.forks.started().len(), first.len() * 2);
    let (fork, _) = harness.fork_submits("keep");
    assert!(!first.iter().any(|(session, _)| *session == fork));

    harness.post_answer(Some("keep"), "");

    assert_eq!(harness.forks.switches(), std::slice::from_ref(&fork));
    assert_eq!(harness.latest_path(), TurnPath::Prepared { session: fork });
}

#[test]
fn after_a_talk_in_the_agent_s_pane_forks_taken_again_hold_it_and_a_bare_answer_uses_one() {
    let mut harness = Harness::start();
    harness.ask(RunAhead::Every);
    let first = harness.forks.started();
    harness.fork_submits("keep");

    // The reviewer talks with the agent in its pane: it works, and its session moves.
    harness.forks.report(AgentStatus::Working);
    harness.forks.move_session("after-the-talk");
    harness.forks.report(AgentStatus::Idle);
    harness.pump();
    let (fork, _) = harness.fork_submits("keep");
    assert!(!first.iter().any(|(session, _)| *session == fork));

    harness.post_answer(Some("keep"), "");

    assert_eq!(harness.forks.switches(), std::slice::from_ref(&fork));
    assert_eq!(harness.latest_path(), TurnPath::Prepared { session: fork });
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
    harness.pump();
    // The closing reviewer waits for the forks of the next question to stop, which the fake
    // stops at once.

    let restored = harness.reopen();

    let prepared = TurnPath::Prepared {
        session: fork.clone(),
    };
    assert_eq!(
        restored.turn_paths,
        [(request.request, prepared.clone())].into()
    );
    assert!(!harness.forks.discarded().contains(&fork));
    let RoundStage::Question { response, .. } = harness.page.stage() else {
        panic!("the page shows the next question");
    };
    assert_eq!(response.path, Some(prepared));
}

#[test]
fn a_turn_saved_while_the_agent_switches_waits_for_it_to_go_back_and_then_reaches_it() {
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
    harness.forks.finish_switch(Ok(switched));
    harness.pump();
    assert_eq!(
        harness.forks.resumes(),
        ["conversation"],
        "the round did not take the fork's turn: the agent goes back to its own session"
    );
    assert!(harness.no_prompt_sent(), "no prompt while it goes back");
    let back = harness.agent_on("conversation");
    harness.forks.finish_resume(Ok(back.clone()));
    harness.pump();

    let prompt = harness.delivered_prompt();
    assert!(prompt.contains(&format!("Explore request: {}\n", again.request)));
    let saved = harness.saved();
    assert_eq!(
        saved.last_agent_session,
        review_explore::ConversationBinding::from_agent(&back),
        "the round follows the session the agent runs"
    );
    let forks = harness.forks_saved();
    assert_eq!(
        forks.answers[0].path,
        TurnPath::Plain {
            reason: PlainReason::Withdrawn
        }
    );
    assert!(matches!(
        forks.forks[0].continued,
        Some(Continuation::Undone { .. })
    ));
    assert!(
        harness.forks.discarded().contains(&fork),
        "the fork the agent left goes"
    );
}

impl Harness {
    /// Answers bare with `choice`, whose fork submitted, so that the agent switches to that
    /// fork; returns the fork's session and the answer's turn.
    fn answer_while_prepared(&mut self, choice: &str) -> (String, TurnRequest) {
        self.ask(RunAhead::Every);
        let (fork, _) = self.fork_submits(choice);
        let request = self.post_answer(Some(choice), "");
        assert_eq!(self.forks.switches(), std::slice::from_ref(&fork));
        (fork, request)
    }

    /// The record of the fork `session`.
    fn fork_record(&self, session: &str) -> ForkRecord {
        self.forks_saved().fork(session).cloned().unwrap()
    }

    /// Retries the turn `request` as the reviewer.
    fn retry(&mut self, request: &TurnRequest) {
        self.session
            .handle(Input::Command(Command::Retry(Box::new(request.clone()))));
    }

    /// Whether `prompt` is the one of the turn `request`.
    fn is_prompt_of(prompt: &str, request: &TurnRequest) -> bool {
        prompt.contains(&format!("Explore request: {}\n", request.request))
    }
}

/// The failure of a switch whose resume Herdr did not confirm in time.
fn unconfirmed() -> SwitchFailure {
    SwitchFailure {
        error: "Herdr did not report the agent on the fork's session".into(),
        typed: true,
    }
}

#[test]
fn a_switch_that_failed_once_the_resume_was_typed_puts_the_agent_back_before_retry_reaches_it() {
    let mut harness = Harness::start();
    let (fork, request) = harness.answer_while_prepared("keep");

    harness.forks.finish_switch(Err(unconfirmed()));
    harness.pump();

    assert!(matches!(
        harness.page.stage(),
        RoundStage::Interrupted { .. }
    ));
    assert_eq!(
        harness.forks.resumes(),
        ["conversation"],
        "the agent, which may still resume the fork's session late, goes back to its own"
    );
    assert!(
        !harness.forks.discarded().contains(&fork),
        "the fork's session may be the agent's until it is back"
    );

    // The reviewer retries before the agent is back: the answer waits for it.
    harness.retry(&request);
    assert!(harness.no_prompt_sent());
    harness.agent_on(&fork);
    let back = harness.agent_on("conversation");
    harness.forks.finish_resume(Ok(back));
    harness.pump();

    let prompt = harness.delivered_prompt();
    assert!(Harness::is_prompt_of(&prompt, &request));
    assert!(
        harness.no_prompt_sent(),
        "the answer reaches the agent once"
    );
    let record = harness.fork_record(&fork);
    assert!(
        matches!(record.continued, Some(Continuation::Undone { .. })),
        "{:?}",
        record.continued
    );
    assert!(harness.forks.discarded().contains(&fork));
    assert!(matches!(
        harness.latest_path(),
        TurnPath::Plain {
            reason: PlainReason::SwitchFailed { .. }
        }
    ));
}

#[test]
fn an_agent_that_could_not_be_put_back_gets_no_prompt_and_retry_tries_again() {
    let mut harness = Harness::start();
    let (fork, request) = harness.answer_while_prepared("keep");
    harness.forks.finish_switch(Err(unconfirmed()));
    harness.pump();
    harness.forks.finish_resume(Err(unconfirmed()));
    harness.pump();
    let _ = harness.next::<ui_events::ExploreFinished>();

    harness.retry(&request);

    assert!(
        harness.no_prompt_sent(),
        "no prompt while the agent's session is uncertain"
    );
    assert_eq!(
        harness.forks.resumes(),
        ["conversation"],
        "Retry tries again"
    );
    harness.forks.finish_resume(Err(unconfirmed()));
    harness.pump();
    let finished = harness.next::<ui_events::ExploreFinished>();
    assert!(finished.result.is_err(), "the turn waits for Retry again");
    assert!(harness.no_prompt_sent());
    assert!(harness.fork_record(&fork).is_unsettled());
    assert!(!harness.forks.discarded().contains(&fork));

    harness.retry(&request);
    harness
        .forks
        .finish_resume(Ok(harness.agent_on("conversation")));
    harness.pump();

    assert!(Harness::is_prompt_of(&harness.delivered_prompt(), &request));
}

#[test]
fn stopping_the_wait_while_the_agent_switches_puts_it_back_and_retry_reaches_it_once() {
    let mut harness = Harness::start();
    let (fork, request) = harness.answer_while_prepared("keep");

    harness.session.handle(Input::Command(Command::Cancel));
    harness.forks.finish_switch(Ok(harness.agent_on(&fork)));
    harness.pump();

    assert_eq!(harness.forks.resumes(), ["conversation"]);
    assert_eq!(
        harness.saved().exploration.conversation.len(),
        1,
        "the fork's turn is not the round's"
    );
    harness.retry(&request);
    assert!(
        harness.no_prompt_sent(),
        "Retry waits for the agent to go back"
    );
    harness
        .forks
        .finish_resume(Ok(harness.agent_on("conversation")));
    harness.pump();

    assert!(Harness::is_prompt_of(&harness.delivered_prompt(), &request));
    assert!(harness.no_prompt_sent());
    assert_eq!(
        harness.latest_path(),
        TurnPath::Plain {
            reason: PlainReason::Withdrawn
        }
    );
}

#[test]
fn a_reset_while_the_agent_switches_puts_it_back() {
    let mut harness = Harness::start();
    let (fork, _) = harness.answer_while_prepared("keep");

    harness.session.handle(Input::Command(Command::Reset));
    harness.forks.finish_switch(Ok(harness.agent_on(&fork)));
    harness.pump();

    assert_eq!(harness.forks.resumes(), ["conversation"]);
    harness
        .forks
        .finish_resume(Ok(harness.agent_on("conversation")));
    harness.pump();
    assert!(harness.no_prompt_sent());
    assert!(matches!(
        harness.fork_record(&fork).continued,
        Some(Continuation::Undone { .. })
    ));
    assert!(harness.forks.discarded().contains(&fork));
}

#[test]
fn a_reopened_reviewer_puts_back_an_agent_left_switching_before_its_answer_goes_out() {
    let mut harness = Harness::start();
    let (fork, _) = harness.answer_while_prepared("keep");
    // The reviewer stops while the agent switches, which Herdr never confirms.
    harness.forks.host().switches.clear();

    let restored = harness.reopen();
    harness.adopt(&restored);
    let request = harness.exploration().retry_request().unwrap().clone();
    harness.retry(&request);

    assert!(harness.no_prompt_sent());
    assert_eq!(harness.forks.resumes(), ["conversation"]);
    harness
        .forks
        .finish_resume(Ok(harness.agent_on("conversation")));
    harness.pump();
    assert!(Harness::is_prompt_of(&harness.delivered_prompt(), &request));
    assert!(matches!(
        harness.fork_record(&fork).continued,
        Some(Continuation::Undone { .. })
    ));
}

#[test]
fn a_reopened_reviewer_keeps_the_agent_on_a_fork_whose_turn_the_round_took() {
    let mut harness = Harness::start();
    let (fork, request) = harness.answer_while_prepared("keep");
    harness.forks.finish_switch(Ok(harness.agent_on(&fork)));
    harness.pump();
    // As a reviewer that stopped once the fork's turn was saved, before it recorded the switch.
    let instance = harness.exploration().instance.clone();
    harness
        .store
        .lock_explore(&harness.unit)
        .unwrap()
        .update_round_forks(&instance, |forks| {
            forks.forks[0].continued = Some(Continuation::Switching { at_ms: 1 });
        })
        .unwrap();
    harness.agent_on("conversation");
    harness.reopen();
    harness.exploration = Some(harness.saved().exploration);

    let next = harness.post_answer(Some("keep"), "Keep it, with a test.");

    assert!(harness.no_prompt_sent());
    assert_eq!(
        harness.forks.switches(),
        std::slice::from_ref(&fork),
        "the agent resumes the fork's session, which holds the round's turn"
    );
    assert!(harness.forks.host().resumes.is_empty());
    harness.forks.finish_switch(Ok(harness.agent_on(&fork)));
    harness.pump();
    assert!(Harness::is_prompt_of(&harness.delivered_prompt(), &next));
    assert!(matches!(
        harness.fork_record(&fork).continued,
        Some(Continuation::Switched { .. })
    ));
    assert!(!harness.forks.discarded().contains(&fork));
    assert!(
        harness
            .saved()
            .exploration
            .conversation
            .iter()
            .any(|turn| turn.update.request == request.request)
    );
}

#[test]
fn cancelling_a_prepared_turn_names_the_answer_by_the_id_its_fork_was_told() {
    let mut harness = Harness::start();
    let (fork, request) = harness.answer_while_prepared("keep");
    let told = fork_line(&harness.fork_prompt(&fork), "Answer ID: ");
    harness.forks.finish_switch(Ok(harness.agent_on(&fork)));
    harness.pump();
    let answer = request.answer.unwrap().id;
    assert_eq!(
        answer, told,
        "the round saved the answer under its fork's ID"
    );

    harness
        .session
        .handle(Input::Command(Command::CancelAnswer(answer.clone())));
    let cancelled = harness.next::<ui_events::ExploreAnswerCancelled>();
    harness.exploration = Some(cancelled.result.unwrap().exploration.clone());
    let again = harness.post_answer(Some("change"), "");

    assert!(
        harness.nothing_moved(),
        "the agent stays on the fork's session"
    );
    let prompt = harness.delivered_prompt();
    assert!(Harness::is_prompt_of(&prompt, &again));
    assert!(
        prompt.contains(&format!("Cancelled answer: {told}\n")),
        "the agent learns that the answer it knows is cancelled: {prompt}"
    );
}

#[test]
fn after_a_prepared_turn_the_agent_reconsiders_the_answer_by_the_id_its_fork_was_told() {
    let mut harness = Harness::start();
    let (fork, request) = harness.answer_while_prepared("keep");
    let told = fork_line(&harness.fork_prompt(&fork), "Answer ID: ");
    harness.forks.finish_switch(Ok(harness.agent_on(&fork)));
    harness.pump();
    assert_eq!(
        harness.latest_path(),
        TurnPath::Prepared {
            session: fork.clone()
        }
    );
    harness.exploration = Some(harness.saved().exploration);

    // The reviewer comments the next question; the agent, on the fork's session, takes the
    // turn and reconsiders the decision of the prepared turn's answer.
    let (next, access) = harness.answer("Keep it, but the first decision needs another look.");
    let Operation::SubmitQuestion(mut update) = question(&next, 3) else {
        unreachable!("a question");
    };
    update.agenda.push(review_explore::AgendaChange {
        topic: "topic1".into(),
        action: review_explore::AgendaAction::Reconsider,
        reason: "The reviewer asked to look again.".into(),
        answer: next.answer.as_ref().map(|answer| answer.id.clone()),
        evidence: Vec::new(),
        replacement: None,
        decision: Some(told.clone()),
    });

    let result = harness.submit(&access, Operation::SubmitQuestion(update));

    assert!(applied(result), "the round accepts the reconsideration");
    assert_eq!(request.answer.unwrap().id, told);
}

#[test]
fn an_answer_with_a_comment_to_a_prepared_choice_keeps_the_ids_its_fork_was_told() {
    let mut harness = Harness::start();
    harness.ask(RunAhead::Every);
    let (fork, _) = harness.fork_submits("keep");
    let prompt = harness.fork_prompt(&fork);

    let request = harness.post_answer(Some("keep"), "Keep it, with a test.");

    assert_eq!(
        harness.latest_path(),
        TurnPath::Plain {
            reason: PlainReason::Comment
        }
    );
    assert_eq!(request.request, fork_line(&prompt, "Explore request: "));
    assert_eq!(
        request.answer.as_ref().unwrap().id,
        fork_line(&prompt, "Answer ID: ")
    );
    assert_eq!(harness.agent_takes(&request), Some(harness.latest_path()));
}

#[test]
fn an_answer_to_a_choice_with_no_fork_keeps_the_ids_its_front_end_gave() {
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
    harness.exploration = Some(harness.saved().exploration);
    let posted = harness.request(Some(AnswerInput {
        option: Some("keep".into()),
        ..AnswerInput::default()
    }));

    let saved = harness.post_turn(&posted);

    assert_eq!(saved.request, posted.request);
    assert_eq!(saved.answer, posted.answer);
}

#[test]
fn no_prompt_of_the_thread_service_reaches_the_agent_while_it_switches_or_goes_back() {
    let mut harness = Harness::start();
    let (fork, _) = harness.answer_while_prepared("keep");
    let prompts = harness.threads.prompt_sender();
    // A round conversation's wakeup goes through the same courier as this prompt.
    let (first, _first) = prompts.send(
        review_thread_service::PinnedAgent::new(agent()),
        "A wakeup during the switch".into(),
    );

    assert!(
        harness.no_prompt_sent(),
        "nothing reaches the agent while it switches"
    );
    harness.session.handle(Input::Command(Command::Cancel));
    harness.forks.finish_switch(Ok(harness.agent_on(&fork)));
    harness.pump();
    assert!(harness.no_prompt_sent(), "nor while it goes back");
    harness
        .forks
        .finish_resume(Ok(harness.agent_on("conversation")));
    harness.pump();

    first.wait().unwrap();
    assert_eq!(
        harness.delivered_prompt(),
        "A wakeup during the switch",
        "it goes out once the agent runs its own session again"
    );
}

/// The fork for "keep" fails as `fail` makes it, which `failure` names: its choice is not
/// prepared, and the answer that picks it runs the plain chain.
fn a_fork_that_fails_leaves_its_choice_unprepared_and_its_answer_runs_the_plain_chain(
    failure: &str,
    fail: impl FnOnce(&mut Harness, &TurnRequest, &str),
) {
    let mut harness = Harness::start();
    let (first, _) = harness.ask(RunAhead::Every);
    let fork = harness.forks_saved().forks[0].clone();
    assert_eq!(fork.choice, "keep");

    fail(&mut harness, &first, &fork.session);
    harness.pump();
    let request = harness.post_answer(Some("keep"), "");

    assert_eq!(
        harness.latest_path(),
        TurnPath::Plain {
            reason: PlainReason::NoTurn
        },
        "{failure}"
    );
    assert!(harness.nothing_moved(), "{failure}");
    assert!(
        Harness::is_prompt_of(&harness.delivered_prompt(), &request),
        "{failure}: the agent in the pane takes the turn"
    );
    let record = harness.fork_record(&fork.session);
    assert!(record.turn.is_none(), "{failure}");
    assert!(
        record.exit.is_some(),
        "{failure}: the record says how it ended"
    );
    let exit = record.exit.as_deref().unwrap();
    assert!(
        harness
            .forks
            .log()
            .iter()
            .any(|line| line.contains(&fork.session) && line.contains(exit)),
        "{failure}: the log says how it ended: {:?}",
        harness.forks.log()
    );
    assert_eq!(harness.forks_saved().failed_in_a_row, 1, "{failure}");
}

#[test]
fn a_fork_that_crashed_leaves_its_choice_unprepared() {
    a_fork_that_fails_leaves_its_choice_unprepared_and_its_answer_runs_the_plain_chain(
        "crashed",
        |harness, _, fork| {
            harness
                .forks
                .end_as(fork, "signal: 6 (SIGABRT): thread panicked", false);
        },
    );
}

#[test]
fn a_rate_limited_fork_leaves_its_choice_unprepared() {
    a_fork_that_fails_leaves_its_choice_unprepared_and_its_answer_runs_the_plain_chain(
        "rate-limited",
        |harness, _, fork| {
            harness.forks.end_as(
                fork,
                "exit status: 1: API Error: 429 rate_limit_error",
                false,
            );
        },
    );
}

#[test]
fn a_fork_that_never_submitted_leaves_its_choice_unprepared() {
    a_fork_that_fails_leaves_its_choice_unprepared_and_its_answer_runs_the_plain_chain(
        "never submitted",
        |harness, _, fork| {
            harness.forks.end_as(fork, "exit status: 0", true);
        },
    );
}

#[test]
fn a_fork_that_submitted_a_refused_turn_leaves_its_choice_unprepared() {
    a_fork_that_fails_leaves_its_choice_unprepared_and_its_answer_runs_the_plain_chain(
        "submitted a refused turn",
        |harness, first, fork| {
            assert!(harness.submit_as_fork(fork, question(first, 2)).is_err());
            harness.forks.end_as(fork, "exit status: 0", true);
        },
    );
}

#[test]
fn too_many_forks_failing_in_a_row_stops_run_ahead_for_the_round_with_a_notice() {
    let mut harness = Harness::start();
    harness.ask(RunAhead::Every);
    for fork in harness.forks_saved().forks {
        harness.forks.end_as(&fork.session, "exit status: 1", false);
    }
    harness.pump();
    let (request, access) = harness.answer("Keep it, with a test.");
    assert!(applied(harness.submit(&access, question(&request, 2))));
    let second: Vec<_> = harness.forks_saved().forks[2..].to_vec();
    assert_eq!(second.len(), 2, "the next question is forked");
    while harness.events.try_recv().is_ok() {}

    harness
        .forks
        .end_as(&second[0].session, "exit status: 1", false);
    harness.pump();

    // The pane shows a notice.
    harness.next::<ui_events::ToastRequested>();
    let forks = harness.forks_saved();
    assert!(forks.halted_at_ms.is_some());
    assert_eq!(
        discarded_for(&forks.forks[3]),
        Some(DiscardReason::TooManyFailures),
        "the forks still running go"
    );
    let next = harness.post_answer(Some("keep"), "");
    assert!(matches!(
        harness.latest_path(),
        TurnPath::Plain {
            reason: PlainReason::NoForks { why: Some(_) }
        }
    ));
    let access = fork_line(&harness.delivered_prompt(), "Explore review access: ");
    assert!(applied(harness.submit(&access, question(&next, 3))));
    assert_eq!(
        harness.forks.started().len(),
        4,
        "no fork is taken for the round again"
    );
}

#[test]
fn a_reviewer_does_not_fork_a_question_that_another_running_reviewer_forks() {
    let mut harness = Harness::start();
    harness.ask(RunAhead::Off);
    let other = RunningProcess::start();
    let instance = harness.exploration().instance.clone();
    harness
        .store
        .lock_explore(&harness.unit)
        .unwrap()
        .update_round_forks(&instance, |forks| {
            forks.forks.push(ForkRecord {
                question: "q1".into(),
                version: 1,
                choice: "keep".into(),
                answer: None,
                session: "another-reviewer-s-fork".into(),
                from: Some("conversation".into()),
                transcripts: "/fake/projects".into(),
                reviewer: other.stamp(),
                process: None,
                taken_at_ms: 1,
                turn: None,
                exit: None,
                usage: None,
                discarded: None,
                cleaned: false,
                continued: None,
            });
        })
        .unwrap();
    harness
        .store
        .save_explore_run_ahead(RunAhead::Every)
        .unwrap();

    harness.session.handle(Input::StorageChanged);

    assert!(
        harness.forks.started().is_empty(),
        "{:?}",
        harness.forks.log()
    );
    harness.post_answer(Some("keep"), "");
    assert!(matches!(
        harness.latest_path(),
        TurnPath::Plain {
            reason: PlainReason::NoForks { why: Some(_) }
        }
    ));
    assert!(
        !harness
            .forks
            .discarded()
            .contains(&"another-reviewer-s-fork".to_owned()),
        "another reviewer's forks stay its own"
    );
}

#[test]
fn a_reopened_reviewer_cleans_up_the_forks_a_stopped_reviewer_left_in_another_review() {
    let mut harness = Harness::start();
    harness.ask(RunAhead::Every);
    let mut comparison = (*harness.exploration().comparison).clone();
    comparison.checkpoint = review_source::ReviewCheckpoint::new("other-review", "checkpoint");
    let round = ExploreRound::new(Exploration::new(Arc::new(comparison)));
    let other: ReviewUnit = "other-review".into();
    let records = harness.store.lock_explore(&other).unwrap();
    records.create_round(&round).unwrap();
    let mut left = harness.forks_saved().forks[0].clone();
    left.session = "left-in-another-review".into();
    left.reviewer = ProcessStamp {
        pid: u32::MAX - 1,
        started: 1,
    };
    records
        .update_round_forks(&round.exploration.instance, |forks| {
            forks.forks.push(left.clone());
        })
        .unwrap();
    drop(records);

    harness.reopen();
    harness.pump();

    assert!(
        harness.forks.discarded().contains(&left.session),
        "{:?}",
        harness.forks.discarded()
    );
    let saved = harness
        .store
        .load_round_forks(&other, &round.exploration.instance)
        .unwrap();
    assert_eq!(
        discarded_for(&saved.forks[0]),
        Some(DiscardReason::ReviewerStopped)
    );
    assert!(saved.forks[0].cleaned);
}

#[test]
fn stopping_the_wait_and_retrying_from_the_explore_page_while_the_agent_switches_reach_it_once() {
    use review_explore_page::{PageCommand, Recovery};
    let mut harness = Harness::start();
    let (fork, request) = harness.answer_while_prepared("keep");

    assert_eq!(harness.stop_on_page(), Ok(()));
    harness.forks.finish_switch(Ok(harness.agent_on(&fork)));
    harness.pump();
    assert_eq!(harness.forks.resumes(), ["conversation"]);
    let retried = harness.on_page(PageCommand::Recover(Recovery::Retry {
        request: request.request.clone(),
        attempt: harness.attempt(),
    }));
    assert_eq!(retried, Ok(()));
    assert!(
        harness.no_prompt_sent(),
        "Retry waits for the agent to go back"
    );
    harness
        .forks
        .finish_resume(Ok(harness.agent_on("conversation")));
    harness.pump();

    assert!(Harness::is_prompt_of(&harness.delivered_prompt(), &request));
    assert!(harness.no_prompt_sent());
}

#[test]
fn a_reset_from_the_explore_page_while_the_agent_switches_puts_it_back() {
    use review_explore_page::{PageCommand, Recovery};
    let mut harness = Harness::start();
    let (fork, _) = harness.answer_while_prepared("keep");
    let round = harness.page.round().expect("a running round");

    let reset = harness.on_page(PageCommand::Recover(Recovery::Reset { round }));

    assert_eq!(reset, Ok(()));
    harness.forks.finish_switch(Ok(harness.agent_on(&fork)));
    harness.pump();
    assert_eq!(harness.forks.resumes(), ["conversation"]);
    harness
        .forks
        .finish_resume(Ok(harness.agent_on("conversation")));
    harness.pump();
    assert!(harness.no_prompt_sent());
    assert!(harness.forks.discarded().contains(&fork));
}
