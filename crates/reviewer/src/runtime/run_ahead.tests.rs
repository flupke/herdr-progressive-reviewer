//! Run-ahead on an isolated Herdr: a Claude Code stand-in in the agent's pane, forked by the
//! reviewer as a real process through `reviewer-control fork-exec`, with the guard as its hook.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use agent_fork::ProcessStamp;
use review_explore::{AnswerInput, Command as ExploreCommand};
use review_explore_round_settings::RunAhead;
use review_run_ahead::{Continuation, DiscardReason, RoundForks};

use super::explore_flow::ExploreFlow;
use super::*;

#[path = "run_ahead/codex.tests.rs"]
mod codex;
#[path = "run_ahead/contract.tests.rs"]
mod contract;

/// The Claude Code configuration directory of the agent and its forks, in the server's
/// directory.
const CLAUDE_CONFIG: &str = "claude-config";

/// The agent a test runs in the agent's pane.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StandIn {
    /// The test agent alone.
    Plain,
    /// A Claude Code stand-in that run-ahead can fork; it and its forks keep their transcripts
    /// under `claude-config` in the server's directory.
    Forkable,
    /// The same, started without the reviewer's Claude Code plugin.
    Unhooked,
}

impl StandIn {
    /// The workspace variables the stand-in reads, under the server's directory `root`.
    pub(super) fn environment(self, root: &Path) -> Vec<String> {
        // Claude Code runs the hooks of the reviewer's plugin.
        let plugin = format!(
            "{}={}",
            variables::HOOKS,
            effects::fixture::test_fork_tools().control.display()
        );
        let configuration = format!("CLAUDE_CONFIG_DIR={}", root.join(CLAUDE_CONFIG).display());
        match self {
            Self::Plain => Vec::new(),
            Self::Forkable => vec![plugin, configuration],
            Self::Unhooked => vec![configuration],
        }
    }
}

/// A round whose agent is the forkable stand-in, with run-ahead on for every choice. The agent
/// holds each turn until the test submitted it.
struct RunAheadFlow {
    flow: ExploreFlow,
}

/// How long the forks' host waits in a test: as in production, but it asks Herdr where the
/// agent stands more often.
fn test_waits() -> review_run_ahead::ForkWaits {
    review_run_ahead::ForkWaits {
        resume_poll: Duration::from_millis(10),
        ..review_run_ahead::ForkWaits::default()
    }
}

impl RunAheadFlow {
    fn start() -> Self {
        Self::start_with(StandIn::Forkable, test_waits())
    }

    /// The flow with the agent `stand_in`, whose forks' host waits as `waits` say.
    fn start_with(stand_in: StandIn, waits: review_run_ahead::ForkWaits) -> Self {
        let repository_files = repository_fixture(RepoType::Git);
        repository_files.write("reviewed.rs", b"pub fn reviewed() {}\n");
        let herdr =
            IsolatedHerdrServer::start_forkable(repository_files.root(), "session", stand_in);
        herdr.hold_turns(true);
        let fixture = ReviewFlowFixture::start_on(repository_files, herdr, |setup| {
            setup.run_ahead.waits = waits;
        });
        fixture
            .runtime
            .store
            .save_explore_run_ahead(RunAhead::Every)
            .unwrap();
        let mut flow = ExploreFlow::start_on(fixture);
        flow.holds_turns = true;
        Self { flow }
    }

    fn herdr(&self) -> &IsolatedHerdrServer {
        &self.flow.fixture.herdr
    }

    fn root(&self) -> &Path {
        self.herdr().server.root()
    }

    /// Waits until `count` forks started, and returns the sessions of the forks whose process
    /// run-ahead recorded.
    fn wait_for_forks(&self, count: usize) -> Vec<String> {
        self.herdr()
            .events()
            .wait_until(&format!("{count} forks"), |received| {
                received
                    .iter()
                    .filter(|reported| matches!(reported.event, StandInEvent::ForkStarted { .. }))
                    .count()
                    >= count
            });
        // Run-ahead records the process of each fork it starts before it takes another input.
        self.flow.fixture.runtime.effects.flush();
        let ready: Vec<_> = self
            .saved()
            .forks
            .iter()
            .filter(|fork| claude_fork::process(&fork.host).is_some())
            .map(|fork| fork.session.clone())
            .collect();
        assert!(
            ready.len() >= count,
            "{} forks started; run-ahead's log:\n{}",
            ready.len(),
            self.log()
        );
        ready
    }

    fn log(&self) -> String {
        fs::read_to_string(self.flow.fixture.runtime.state.path().join("run-ahead.log"))
            .unwrap_or_default()
    }

    fn saved(&self) -> RoundForks {
        self.flow
            .fixture
            .runtime
            .store
            .load_round_forks(
                &self.flow.fixture.review_unit,
                &self.flow.exploration.instance,
            )
            .unwrap()
    }

    /// What `check` finds in the saved forks, once it finds something.
    fn wait_for_saved<T>(&self, what: &str, check: impl Fn(&RoundForks) -> Option<T>) -> T {
        self.flow.state.wait_until(
            what,
            || check(&self.saved()),
            || format!("run-ahead's log:\n{}", self.log()),
        )
    }

    fn fork_file(&self, session: &str, kind: &str) -> String {
        fs::read_to_string(self.herdr().directory().fork_record(session, kind)).unwrap()
    }

    fn transcript(&self, session: &str) -> PathBuf {
        stand_in_agent::transcript(&self.root().join(CLAUDE_CONFIG), session)
    }

    /// Waits until run-ahead stopped the forks `forks` and deleted their transcripts.
    fn wait_until_gone(&self, forks: &[(String, ProcessStamp)]) {
        self.wait_for_saved("the forks' end", |saved| {
            forks
                .iter()
                .all(|(session, _)| {
                    saved
                        .forks
                        .iter()
                        .any(|fork| fork.session == *session && fork.cleaned)
                })
                .then_some(())
        });
        for (session, process) in forks {
            assert!(
                !process.is_running(),
                "the fork {session} outlived its question"
            );
            assert!(!self.transcript(session).exists());
        }
    }

    /// The sessions and processes of the saved forks.
    fn processes(&self) -> Vec<(String, ProcessStamp)> {
        self.saved()
            .forks
            .iter()
            .map(|fork| {
                (
                    fork.session.clone(),
                    claude_fork::process(&fork.host).expect("a started fork"),
                )
            })
            .collect()
    }
}

fn prompt_line(prompt: &str, label: &str) -> String {
    prompt
        .lines()
        .find_map(|line| line.strip_prefix(label))
        .unwrap_or_else(|| panic!("no {label:?} in the prompt"))
        .to_owned()
}

/// The process group of the running process `pid`.
fn process_group(pid: u32) -> String {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    let (_, fields) = stat.rsplit_once(')').unwrap();
    fields.split_whitespace().nth(2).unwrap().to_owned()
}

/// The next question, as the fork told the prompt `prompt` submits it: its interpretation keeps
/// the outcome of the choice it was told.
fn fork_question(flow: &ExploreFlow, prompt: &str) -> serde_json::Value {
    let choice = prompt_line(prompt, "Selected option ID: ");
    let outcome = flow
        .exploration
        .questions
        .last()
        .and_then(|question| {
            question
                .alternatives
                .iter()
                .find(|alternative| alternative.id == choice)
        })
        .map(|alternative| alternative.outcome)
        .expect("the fork's choice");
    serde_json::json!({
        "instance": prompt_line(prompt, "Explore round: "),
        "request": prompt_line(prompt, "Explore request: "),
        "checkpoint": flow.exploration.comparison.checkpoint,
        "interpretation": {"answer": prompt_line(prompt, "Answer ID: "), "status": outcome,
            "recap": "Keep resolved conversations resolved.", "follow_ups": []},
        "reply": {"text": "I checked the policy.", "evidence": []},
        "topics": [{"id": "topic2", "title": "Policy consequence", "entries": [{"path": "reviewed.rs", "side": "new", "lines": null}], "status": "open"}],
        "next": {"id": "q2", "version": 1, "topic": "topic2", "text": "Keep resolved conversations resolved?",
            "rationale": null, "visual": null,
            "alternatives": [{"id": "keep", "text": "Keep resolved", "outcome": "accepted"}, {"id": "change", "text": "Reopen", "outcome": "needs_follow_up"}],
            "evidence": [{"path": "reviewed.rs", "side": "new", "lines": {"first_line": 1, "last_line": 1}, "notes": "Implements the policy that determines whether completed conversations should reopen"}]},
        "design": null, "conclusion": null, "limitations": [], "findings": []
    })
}

#[test]
fn forks_of_the_agent_take_each_answer_s_turn_which_is_kept_and_an_answer_stops_them() {
    let mut run = RunAheadFlow::start();
    run.flow.turn(None, 1);
    let pane_access = run.flow.access.clone();

    let sessions = run.wait_for_forks(2);

    let saved = run.saved();
    let choices: Vec<_> = saved
        .forks
        .iter()
        .map(|fork| fork.choice.as_str())
        .collect();
    assert_eq!(choices, ["keep", "change"]);
    for (session, fork) in sessions.iter().zip(&saved.forks) {
        let arguments: Vec<String> = run
            .fork_file(session, "args")
            .lines()
            .map(str::to_owned)
            .collect();
        let has = |pair: &[&str]| arguments.windows(pair.len()).any(|window| window == pair);
        assert!(has(&["--resume", "session"]), "{arguments:?}");
        assert!(has(&["--fork-session"]));
        assert!(has(&["--session-id", session]));
        assert!(has(&["--model", "stand-in-model"]));
        let settings = &arguments[arguments.iter().position(|a| a == "--settings").unwrap() + 1];
        assert!(settings.contains("fork-guard"), "{settings}");
        assert!(
            run.fork_file(session, "env")
                .lines()
                .all(|name| !name.starts_with("HERDR_")),
            "a fork carries no Herdr variable"
        );
        let prompt = run.fork_file(session, "prompt");
        assert_eq!(prompt_line(&prompt, "Selected option ID: "), fork.choice);
        assert_ne!(prompt_line(&prompt, "Explore review access: "), pane_access);
        let process = claude_fork::process(&fork.host).unwrap();
        assert_eq!(
            process_group(process.pid),
            process_group(std::process::id()),
            "a fork stays in the reviewer's process group"
        );
    }

    // The fork for "keep" submits its turn, with its own access: kept, and shown to nobody.
    let keep = run.fork_file(&sessions[0], "prompt");
    let questions = run.flow.exploration.questions.len();
    run.flow.access = prompt_line(&keep, "Explore review access: ");
    let kept = run.flow.submit(&fork_question(&run.flow, &keep));
    assert_ne!(kept.is_error, Some(true), "{kept:?}");
    assert_eq!(run.flow.exploration.questions.len(), questions);
    assert!(run.saved().forks[0].turn.is_some());
    run.flow.access = pane_access;

    // Its turn done, it ends; the reviewer keeps how and its tokens.
    run.herdr().events().send(
        &StandInRole::Fork {
            session: sessions[0].clone(),
        },
        &StandInCommand::End,
    );
    let usage = run.wait_for_saved("the fork's end", |saved| saved.forks[0].usage);
    assert_eq!(usage.cache_read, 1000);

    let processes = run.processes();
    run.flow.turn(
        Some(AnswerInput {
            option: Some("keep".into()),
            text: "Keep it, with a regression test.".into(),
            in_reply_to: None,
            first_pick: None,
            ..AnswerInput::default()
        }),
        2,
    );

    run.wait_until_gone(&processes);
    assert!(
        run.transcript("session").exists(),
        "the agent's own transcript stays"
    );
    let saved = run.saved();
    assert!(saved.forks[..2].iter().all(|fork| {
        fork.discarded.as_ref().map(|discard| discard.reason) == Some(DiscardReason::Answered)
    }));
}

impl RunAheadFlow {
    /// The fork `session` submits the turn its prompt asks for, with its own access; its output
    /// then says that its submit has its answer.
    fn fork_submits(&mut self, session: &str) {
        let prompt = self.fork_file(session, "prompt");
        let pane_access = std::mem::replace(
            &mut self.flow.access,
            prompt_line(&prompt, "Explore review access: "),
        );
        let kept = self.flow.submit(&fork_question(&self.flow, &prompt));
        assert_ne!(kept.is_error, Some(true), "{kept:?}");
        self.flow.access = pane_access;
        let fork = StandInRole::Fork {
            session: session.to_owned(),
        };
        self.herdr().events().send(&fork, &StandInCommand::Submit);
        self.herdr()
            .events()
            .wait_until("the fork's submit", |received| {
                received.iter().any(|reported| {
                    reported.from == fork && reported.event == StandInEvent::ForkSubmitted
                })
            });
    }

    /// The reviewer answers the latest question with `choice` and no comment, in the pane.
    fn answer_bare(&mut self, choice: &str) -> review_explore::TurnRequest {
        let question = self.flow.exploration.questions.last().cloned();
        let request = self
            .flow
            .exploration
            .request(
                Some(AnswerInput {
                    option: Some(choice.into()),
                    text: String::new(),
                    in_reply_to: None,
                    first_pick: None,
                    ..AnswerInput::default()
                }),
                question.as_ref(),
            )
            .unwrap();
        self.flow.post_turn(&request)
    }

    /// Waits until the saved round has `count` questions, and returns it.
    fn wait_for_questions(&self, count: usize) -> review_explore::ExploreRound {
        self.flow.state.wait_until(
            &format!("{count} questions"),
            || {
                let saved = self.flow.saved();
                (saved.exploration.questions.len() == count).then_some(saved)
            },
            || {
                format!(
                    "the round has {} questions; run-ahead's log:\n{}",
                    self.flow.saved().exploration.questions.len(),
                    self.log()
                )
            },
        )
    }

    /// The sessions the agent in the pane resumed, in order, once the reviewer sent every
    /// prompt its inputs so far lead to and the agent read what was typed in its pane. A
    /// `/resume` that a thread of the forks' host types later is not among them.
    fn resumed(&self) -> Vec<String> {
        self.flow.fixture.settle();
        self.herdr().resumed()
    }

    /// Checks that the answer `request`, which waited for its fork, reached the agent in the pane
    /// on its own session as the prompt after the first `before`, and that its path says
    /// `reason`.
    fn answer_reached_the_agent(
        &self,
        before: usize,
        request: &review_explore::TurnRequest,
        reason: review_run_ahead::PlainReason,
    ) {
        let delivered = self
            .herdr()
            .wait_for_prompts("the answer", |delivered| delivered.len() > before);
        assert!(
            delivered[before].contains(&format!("Explore request: {}", request.request)),
            "{delivered:?}"
        );
        assert_eq!(self.resumed(), Vec::<String>::new());
        assert_eq!(
            self.saved().answers.last().unwrap().path,
            review_run_ahead::TurnPath::Plain { reason }
        );
    }
}

#[test]
fn a_bare_answer_continues_as_its_fork_and_the_next_answer_reaches_the_fork_s_session() {
    let mut run = RunAheadFlow::start();
    run.flow.turn(None, 1);
    let sessions = run.wait_for_forks(2);
    let (fork, other) = (sessions[0].clone(), sessions[1].clone());
    run.fork_submits(&fork);
    let gone = run.processes()[1..].to_vec();
    let prompts = run.herdr().prompts();

    let request = run.answer_bare("keep");

    let saved = run.wait_for_questions(2);
    assert_eq!(run.resumed(), std::slice::from_ref(&fork));
    let agent = run
        .herdr()
        .client()
        .get_agent(&run.herdr().pane_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        agent.agent_session.as_ref().map(|session| &session.value),
        Some(&fork),
        "Herdr reports the agent in the pane on the fork's session"
    );
    assert_eq!(
        saved.last_agent_session,
        review_explore::ConversationBinding::from_agent(&agent)
    );
    let turn = &saved.exploration.conversation.last().unwrap().update;
    assert_eq!(turn.request, request.request);
    assert_eq!(
        turn.interpretation.as_ref().unwrap().answer,
        request.answer.as_ref().unwrap().id
    );
    assert_eq!(
        run.herdr().prompts(),
        prompts,
        "the agent in the pane got no prompt"
    );
    run.wait_until_gone(&gone);
    assert!(!run.transcript(&other).exists());
    assert!(
        run.transcript(&fork).exists(),
        "the fork's session is the agent's now"
    );
    let record = &run.saved().forks[0];
    assert!(record.discarded.is_none() && !record.cleaned);

    // The next answer goes to the agent, which runs the fork's session, and its turn is
    // accepted from that session.
    run.flow.exploration = saved.exploration;
    run.flow.turn(
        Some(AnswerInput {
            option: Some("change".into()),
            text: "Reopen them, and say so.".into(),
            in_reply_to: None,
            first_pick: None,
            ..AnswerInput::default()
        }),
        3,
    );
    assert!(
        fs::read_to_string(run.transcript(&fork))
            .unwrap()
            .contains("\"uuid\":\"prompt-2\""),
        "the prompt reached the fork's session"
    );
    // The forks of the second question, then the first of the third's.
    let third = run.wait_for_forks(5)[4].clone();
    assert!(
        run.fork_file(&third, "args")
            .lines()
            .collect::<Vec<_>>()
            .windows(2)
            .any(|pair| pair == ["--resume", fork.as_str()]),
        "forks of the next question copy the fork's session"
    );
    // The agent resumed no other session: a settling move would type its `/resume` on a thread
    // of its own, which no flush waits for, but run-ahead takes no forks while one runs, and it
    // ends only once Herdr reports the agent where the `/resume` put it.
    assert_eq!(run.resumed(), std::slice::from_ref(&fork));

    // Once the reviewer closes, the agent's project folder holds the agent's sessions only.
    let RunAheadFlow { flow } = run;
    let ExploreFlow { fixture, .. } = flow;
    let ReviewFlowFixture { runtime, herdr, .. } = fixture;
    drop(runtime);
    assert_eq!(
        project_folder(&herdr.server.root().join(CLAUDE_CONFIG)),
        BTreeSet::from(["session.jsonl".to_owned(), format!("{fork}.jsonl")]),
    );
}

#[test]
fn a_bare_answer_before_its_fork_submitted_waits_and_the_next_answer_reaches_the_fork_s_session() {
    let mut run = RunAheadFlow::start();
    run.flow.turn(None, 1);
    let sessions = run.wait_for_forks(2);
    let fork = sessions[0].clone();
    let other = run.processes()[1..].to_vec();
    let prompts = run.herdr().prompts();

    // The reviewer answers while the fork for that choice still works.
    let request = run.answer_bare("keep");

    run.wait_until_gone(&other);
    assert_eq!(run.flow.saved().exploration.questions.len(), 1);
    assert_eq!(run.resumed(), Vec::<String>::new(), "the agent waits");
    assert_eq!(run.herdr().prompts(), prompts, "the agent got no prompt");

    // The fork submits, still running: the agent switches to it at once.
    run.fork_submits(&fork);
    let saved = run.wait_for_questions(2);
    assert_eq!(
        saved
            .exploration
            .conversation
            .last()
            .unwrap()
            .update
            .request,
        request.request
    );
    // The reviewer answers the fork's question right away.
    run.flow.exploration = saved.exploration;
    run.flow.turn(
        Some(AnswerInput {
            option: Some("change".into()),
            text: "Reopen them, and say so.".into(),
            in_reply_to: None,
            first_pick: None,
            ..AnswerInput::default()
        }),
        3,
    );

    assert!(
        fs::read_to_string(run.transcript(&fork))
            .unwrap()
            .contains("\"uuid\":\"prompt-2\""),
        "the answer reached the fork's session, which holds the fork's turn"
    );
    assert_eq!(run.resumed(), std::slice::from_ref(&fork));
    assert_eq!(run.herdr().prompts().len(), prompts.len() + 1);
}

#[test]
fn stop_waiting_for_a_fork_still_at_work_stops_it_and_gives_the_answer_to_the_agent() {
    let mut run = RunAheadFlow::start();
    run.flow.turn(None, 1);
    run.wait_for_forks(2);
    let processes = run.processes();
    let prompts = run.herdr().prompts().len();
    let request = run.answer_bare("keep");

    run.flow.fixture.explore(ExploreCommand::Cancel);

    run.wait_until_gone(&processes);
    run.answer_reached_the_agent(
        prompts,
        &request,
        review_run_ahead::PlainReason::StoppedWaiting,
    );
}

#[test]
fn a_fork_that_ends_without_its_turn_gives_the_answer_to_the_agent() {
    let mut run = RunAheadFlow::start();
    run.flow.turn(None, 1);
    let fork = run.wait_for_forks(2)[0].clone();
    let prompts = run.herdr().prompts().len();
    let request = run.answer_bare("keep");

    run.herdr()
        .events()
        .send(&StandInRole::Fork { session: fork }, &StandInCommand::End);

    run.answer_reached_the_agent(prompts, &request, review_run_ahead::PlainReason::NoTurn);
}

#[test]
fn a_switch_the_agent_never_confirms_puts_the_agent_back_and_retry_reaches_its_own_session_once() {
    // The switch waits no time, in place of 20 seconds: neither the agent's hook nor Herdr
    // confirms it. The agent's move back to its own session, which they confirm, waits as long
    // as in production.
    let mut run = RunAheadFlow::start_with(
        StandIn::Forkable,
        review_run_ahead::ForkWaits {
            resume: Duration::ZERO,
            ..test_waits()
        },
    );
    run.flow.turn(None, 1);
    let sessions = run.wait_for_forks(2);
    let fork = sessions[0].clone();
    run.fork_submits(&fork);
    fs::write(run.herdr().directory().unreported_resume(), "").unwrap();
    let prompts = run.herdr().prompts().len();

    let request = run.answer_bare("keep");

    // The switch waits in vain, then the agent goes back to its own session.
    run.wait_for_saved("the switch undone", |saved| {
        matches!(saved.forks[0].continued, Some(Continuation::Undone { .. })).then_some(())
    });
    assert_eq!(run.resumed(), [fork.clone(), "session".to_owned()]);
    assert_eq!(
        run.herdr().prompts().len(),
        prompts,
        "the agent in the pane got no prompt yet"
    );
    run.wait_until_gone(&run.processes()[..1]);
    let saved = run.wait_for_questions(1);
    assert_eq!(saved.exploration.conversation.len(), 1);

    run.flow
        .fixture
        .explore(ExploreCommand::Retry(Box::new(request.clone())));

    let delivered = run
        .herdr()
        .wait_for_prompts("the retried answer", |delivered| delivered.len() > prompts);
    let delivered = &delivered[prompts..];
    let explore_request = format!("Explore request: {}", request.request);
    assert!(delivered[0].contains(&explore_request), "{delivered:?}");
    assert!(
        fs::read_to_string(run.transcript("session"))
            .unwrap()
            .contains("\"uuid\":\"prompt-2\""),
        "the answer reached the agent's own session"
    );
    // A settling move that started meanwhile holds the prompt until its `/resume` is read.
    assert_eq!(run.resumed(), [fork.clone(), "session".to_owned()]);
    assert_eq!(
        run.herdr()
            .prompts()
            .iter()
            .filter(|prompt| prompt.contains(&explore_request))
            .count(),
        1,
        "the answer reached the agent once"
    );
}

#[test]
fn text_left_in_the_input_box_is_blocked_with_the_resume_and_the_answer_waits_for_retry() {
    let mut run = RunAheadFlow::start();
    run.flow.turn(None, 1);
    let sessions = run.wait_for_forks(2);
    let fork = sessions[0].clone();
    run.fork_submits(&fork);
    let processes = run.processes();
    let prompts = run.herdr().prompts().len();
    // The reviewer left half a thought in the agent's input box.
    run.herdr().run_cli(&[
        "pane",
        "send-text",
        &run.herdr().pane_id.0,
        "half a thought ",
    ]);

    let request = run.answer_bare("keep");

    let blocked = run
        .herdr()
        .events()
        .wait_for("the blocked prompt", |reported| {
            matches!(reported.event, StandInEvent::PromptBlocked { .. })
        });
    assert_eq!(
        blocked.event,
        StandInEvent::PromptBlocked {
            text: format!("half a thought /resume {fork}")
        }
    );
    // The agent put the joined prompt back in its input box, which its user empties.
    run.herdr()
        .run_cli(&["pane", "send-keys", &run.herdr().pane_id.0, "ctrl+u"]);
    let typed = run.wait_for_saved("the failed switch", |saved| {
        match &saved.forks[0].continued {
            Some(Continuation::Failed { typed, .. }) => Some(*typed),
            _ => None,
        }
    });
    assert!(!typed, "the agent ran neither the text nor the /resume");
    run.wait_until_gone(&processes);
    assert_eq!(run.resumed(), Vec::<String>::new());
    assert_eq!(
        run.herdr().prompts().len(),
        prompts,
        "the agent took no turn"
    );
    assert!(matches!(
        &run.saved().answers.last().unwrap().path,
        review_run_ahead::TurnPath::Plain {
            reason: review_run_ahead::PlainReason::SwitchFailed { .. }
        }
    ));

    run.flow
        .fixture
        .explore(ExploreCommand::Retry(Box::new(request.clone())));

    let delivered = run
        .herdr()
        .wait_for_prompts("the retried answer", |delivered| delivered.len() > prompts);
    assert!(
        delivered[prompts].contains(&format!("Explore request: {}", request.request)),
        "{delivered:?}"
    );
}

#[test]
fn an_agent_started_without_the_plugin_is_not_forked_and_its_answers_say_to_restart_it() {
    let mut run = RunAheadFlow::start_with(StandIn::Unhooked, test_waits());
    run.flow.turn(None, 1);
    let prompts = run.herdr().prompts().len();

    let request = run.answer_bare("keep");

    let delivered = run
        .herdr()
        .wait_for_prompts("the answer", |delivered| delivered.len() > prompts);
    assert!(
        delivered[prompts].contains(&format!("Explore request: {}", request.request)),
        "the answer reached the agent: {delivered:?}"
    );
    let saved = run.saved();
    assert_eq!(saved.forks, Vec::new(), "{}", run.log());
    assert_eq!(
        saved.answers.last().unwrap().path,
        review_run_ahead::TurnPath::Plain {
            reason: review_run_ahead::PlainReason::NoHooks
        }
    );
}

#[test]
fn a_reset_stops_every_fork_and_deletes_its_transcript() {
    let mut run = RunAheadFlow::start();
    run.flow.turn(None, 1);
    run.wait_for_forks(2);
    let processes = run.processes();

    run.flow.fixture.explore(ExploreCommand::Reset);

    run.wait_until_gone(&processes);
}

#[test]
fn a_message_in_the_round_s_conversation_stops_every_fork() {
    let mut run = RunAheadFlow::start();
    run.flow.turn(None, 1);
    run.wait_for_forks(2);
    let processes = run.processes();

    run.flow
        .fixture
        .runtime
        .perform([Action::Thread(review_threads::ThreadCommand::Post {
            review_unit: run.flow.fixture.review_unit.clone(),
            post: review_threads::Post::to_round(
                &run.flow.exploration.instance,
                "Why does the policy exist?".into(),
                None,
                None,
            ),
        })]);

    run.wait_until_gone(&processes);
    assert!(run.saved().forks.iter().all(|fork| {
        fork.discarded.as_ref().map(|discard| discard.reason) == Some(DiscardReason::ChatMessage)
    }));
}

#[test]
fn cancelling_the_answer_stops_the_forks_of_the_question_it_led_to() {
    let mut run = RunAheadFlow::start();
    run.flow.turn(None, 1);
    run.wait_for_forks(2);
    run.flow.turn(
        Some(AnswerInput {
            option: Some("keep".into()),
            text: "Keep it, with a regression test.".into(),
            in_reply_to: None,
            first_pick: None,
            ..AnswerInput::default()
        }),
        2,
    );
    run.wait_for_forks(4);
    let second = run.processes()[2..].to_vec();
    let answer = run.flow.exploration.answers.last().unwrap().id.clone();

    run.flow
        .fixture
        .explore(ExploreCommand::CancelAnswer(answer));

    run.wait_until_gone(&second);
}

#[test]
fn a_closing_reviewer_stops_every_fork_and_deletes_its_transcript() {
    let mut run = RunAheadFlow::start();
    run.flow.turn(None, 1);
    run.wait_for_forks(2);
    let processes = run.processes();
    let RunAheadFlow { flow } = run;
    let ExploreFlow { fixture, .. } = flow;
    let ReviewFlowFixture { runtime, herdr, .. } = fixture;

    drop(runtime);

    for (session, process) in &processes {
        assert!(
            !process.is_running(),
            "the fork {session} outlived the reviewer"
        );
    }
    assert_eq!(
        project_folder(&herdr.server.root().join(CLAUDE_CONFIG)),
        BTreeSet::from(["session.jsonl".to_owned()]),
        "only the agent's own session is left"
    );
}

/// What the project folder of the stand-in's transcripts under `configuration` holds.
fn project_folder(configuration: &Path) -> BTreeSet<String> {
    fs::read_dir(stand_in_agent::transcripts(configuration))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect()
}
