//! Run-ahead on an isolated Herdr: a Claude Code stand-in in the agent's pane, forked by the
//! reviewer as a real process through `reviewer-control fork-exec`, with the guard as its hook.

use std::io::Read as _;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use agent_fork::ProcessStamp;
use review_explore::{AnswerInput, Command as ExploreCommand};
use review_explore_round_settings::RunAhead;
use review_run_ahead::{Continuation, DiscardReason, RoundForks};

use super::explore_flow::ExploreFlow;
use super::*;

/// The agent a test runs in the agent's pane.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StandIn {
    /// The test agent alone.
    Plain,
    /// A Claude Code stand-in that run-ahead can fork; it and its forks keep their transcripts
    /// under `claude-config` in the server's directory.
    Forkable,
    /// The same, whose forks run the real Claude Code named in `real-claude` in the server's
    /// directory, with the owner's own configuration and transcripts.
    ForkedForReal,
}

impl StandIn {
    /// The workspace variables the stand-in reads, under the server's directory `root`.
    pub(super) fn environment(self, root: &Path) -> Vec<String> {
        let stand_in = format!(
            "REVIEW_AGENT_E2E_STANDIN={}",
            root.join("standin").display()
        );
        match self {
            Self::Plain => Vec::new(),
            Self::Forkable => vec![
                stand_in,
                format!("CLAUDE_CONFIG_DIR={}", root.join("claude-config").display()),
            ],
            Self::ForkedForReal => vec![stand_in],
        }
    }

    /// Installs the stand-in as `script`: a shell script named `claude`, the process Herdr
    /// detects, which runs the test agent, or the fork stand-in when it is started as a fork.
    pub(super) fn install(root: &Path, script: &Path) {
        fs::copy(std::env::current_exe().unwrap(), root.join("standin")).unwrap();
        fs::write(
            script,
            "#!/bin/sh\n\
             case \" $* \" in\n\
             *\" --fork-session \"*)\n  \
             REVIEW_AGENT_E2E_FORK_ARGS=$(printf '%s\\n' \"$@\") exec \"$REVIEW_AGENT_E2E_STANDIN\" \
             --exact runtime::tests::run_ahead::e2e_fork_process --nocapture ;;\n\
             esac\n\
             \"$REVIEW_AGENT_E2E_STANDIN\" --exact runtime::tests::e2e_agent_process --nocapture\n",
        )
        .unwrap();
        fs::set_permissions(script, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// The projects directory of the stand-in's transcripts, from its environment.
fn transcripts() -> Option<PathBuf> {
    std::env::var_os("CLAUDE_CONFIG_DIR").map(|config| PathBuf::from(config).join("projects"))
}

/// The transcript the test agent keeps for its session when it stands in for Claude Code, as
/// Claude Code does: a line per prompt it read and per answer it gave.
pub(super) struct StandInTranscript {
    path: Option<PathBuf>,
    turns: usize,
}

impl StandInTranscript {
    /// The transcript of the agent's session; none when the agent keeps no transcript.
    pub(super) fn open() -> Self {
        let path = std::env::var("REVIEW_AGENT_E2E_AGENT_SESSION")
            .ok()
            .zip(transcripts())
            .map(|(session, projects)| {
                let directory = projects.join("standin");
                fs::create_dir_all(&directory).unwrap();
                let path = directory.join(format!("{session}.jsonl"));
                fs::write(&path, "{\"type\":\"user\",\"uuid\":\"start\"}\n").unwrap();
                path
            });
        Self { path, turns: 0 }
    }

    /// The agent continues the session `session`, whose transcript a fork wrote.
    pub(super) fn resume(&mut self, session: &str) {
        if let Some(path) = &mut self.path {
            path.set_file_name(format!("{session}.jsonl"));
        }
    }

    /// The agent read a prompt and answered it.
    pub(super) fn turn(&mut self) {
        let Some(path) = &self.path else {
            return;
        };
        self.turns += 1;
        let mut file = File::options().append(true).open(path).unwrap();
        writeln!(
            file,
            "{{\"type\":\"user\",\"uuid\":\"prompt-{0}\"}}\n\
             {{\"type\":\"assistant\",\"uuid\":\"answer-{0}\",\"message\":{{\"model\":\"stand-in-model\"}}}}",
            self.turns
        )
        .unwrap();
    }
}

/// Records beside the test agent's prompts, at `prompt_path`, that the agent resumed the
/// session `session`.
pub(super) fn record_resume(prompt_path: &std::ffi::OsStr, session: &str) {
    let mut resumes = File::options()
        .create(true)
        .append(true)
        .open(resumes_path(Path::new(prompt_path)))
        .unwrap();
    writeln!(resumes, "{session}").unwrap();
}

fn resumes_path(prompt_path: &Path) -> PathBuf {
    prompt_path.with_file_name("resumes.txt")
}

/// A fork of the stand-in: it records its arguments, environment and prompt beside the test
/// agent's prompts, copies its parent's transcript, then works until the test ends its turn or
/// the reviewer stops it. Once the test submitted the fork's turn, the fork's output says that
/// its submit has its answer.
#[test]
fn e2e_fork_process() {
    let Ok(arguments) = std::env::var("REVIEW_AGENT_E2E_FORK_ARGS") else {
        return;
    };
    let arguments: Vec<&str> = arguments.lines().collect();
    let value = |name: &str| {
        let at = arguments
            .iter()
            .position(|argument| *argument == name)
            .unwrap();
        arguments[at + 1].to_owned()
    };
    let real = PathBuf::from(std::env::var_os("REVIEW_AGENT_E2E_PROMPT_PATH").unwrap())
        .with_file_name("real-claude");
    if let Ok(real) = fs::read_to_string(real) {
        // The real Claude Code takes the fork's turn, with the pane's flags the test gives it.
        let mut real = real.lines();
        let program = real.next().unwrap();
        let from = arguments
            .iter()
            .position(|argument| *argument == "-p")
            .unwrap();
        let error = Command::new(program)
            .args(real)
            .args(&arguments[from..])
            .exec();
        panic!("could not run {program}: {error}");
    }
    let (session, parent) = (value("--session-id"), value("--resume"));
    let mut prompt = String::new();
    io::stdin().read_to_string(&mut prompt).unwrap();
    let forks = forks_directory(&PathBuf::from(
        std::env::var_os("REVIEW_AGENT_E2E_PROMPT_PATH").unwrap(),
    ));
    fs::create_dir_all(&forks).unwrap();
    let directory = transcripts().unwrap().join("standin");
    let mut history = fs::read_to_string(directory.join(format!("{parent}.jsonl"))).unwrap();
    history.push_str("{\"type\":\"user\",\"uuid\":\"fork-prompt\"}\n");
    fs::write(directory.join(format!("{session}.jsonl")), history).unwrap();
    let names: Vec<_> = std::env::vars_os()
        .map(|(name, _)| name.to_string_lossy().into_owned())
        .collect();
    fs::write(forks.join(format!("{session}.args")), arguments.join("\n")).unwrap();
    fs::write(forks.join(format!("{session}.env")), names.join("\n")).unwrap();
    fs::write(forks.join(format!("{session}.prompt")), prompt).unwrap();
    let usage = r#"{"input_tokens":3,"cache_creation_input_tokens":20,"cache_read_input_tokens":1000,"output_tokens":7}"#;
    println!(r#"{{"type":"system","subtype":"init","session_id":"{session}"}}"#);
    println!(r#"{{"type":"assistant","message":{{"id":"m1","usage":{usage}}}}}"#);
    io::stdout().flush().unwrap();
    let mut answered = false;
    while !forks.join(format!("{session}.end")).exists() {
        if !answered && forks.join(format!("{session}.submitted")).exists() {
            answered = true;
            println!(
                r#"{{"type":"assistant","message":{{"id":"m2","content":[{{"type":"tool_use","id":"submit","name":"mcp__herdr_reviewer__submit_question","input":{{}}}}],"usage":{usage}}}}}"#
            );
            println!(
                r#"{{"type":"user","message":{{"content":[{{"type":"tool_result","tool_use_id":"submit"}}]}}}}"#
            );
            io::stdout().flush().unwrap();
        }
        thread::sleep(Duration::from_millis(20));
    }
    println!(r#"{{"type":"result","subtype":"success","is_error":false}}"#);
}

fn forks_directory(prompt_path: &Path) -> PathBuf {
    prompt_path.with_file_name("forks")
}

/// A round whose agent is the forkable stand-in, with run-ahead on for every choice.
struct RunAheadFlow {
    flow: ExploreFlow,
}

impl RunAheadFlow {
    fn start() -> Self {
        let repository_files = repository_fixture(RepoType::Git);
        repository_files.write("reviewed.rs", b"pub fn reviewed() {}\n");
        let herdr = IsolatedHerdrServer::start_forkable(
            repository_files.root(),
            "session",
            StandIn::Forkable,
        );
        herdr.show_idle();
        let fixture = ReviewFlowFixture::start_on(repository_files, herdr);
        fixture
            .runtime
            .store
            .save_explore_run_ahead(RunAhead::Every)
            .unwrap();
        Self {
            flow: ExploreFlow::start_on(fixture),
        }
    }

    fn root(&self) -> &Path {
        self.flow.fixture.herdr.server.root()
    }

    fn forks(&self) -> PathBuf {
        forks_directory(&self.root().join("prompt.txt"))
    }

    /// Waits until `count` forks have their prompt, and returns their sessions.
    fn wait_for_forks(&self, count: usize) -> Vec<String> {
        let deadline = Instant::now() + HERDR_WAIT;
        loop {
            let saved = self.saved();
            let ready: Vec<_> = saved
                .forks
                .iter()
                .map(|fork| fork.session.clone())
                .filter(|session| self.forks().join(format!("{session}.prompt")).exists())
                .collect();
            if ready.len() >= count {
                return ready;
            }
            assert!(
                Instant::now() < deadline,
                "{} forks started; run-ahead's log:\n{}",
                ready.len(),
                self.log()
            );
            thread::sleep(Duration::from_millis(25));
        }
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

    fn fork_file(&self, session: &str, kind: &str) -> String {
        fs::read_to_string(self.forks().join(format!("{session}.{kind}"))).unwrap()
    }

    fn transcript(&self, session: &str) -> PathBuf {
        self.root()
            .join("claude-config/projects/standin")
            .join(format!("{session}.jsonl"))
    }

    /// Waits until no fork process runs and no fork transcript is left.
    fn wait_until_gone(&self, forks: &[(String, ProcessStamp)]) {
        let deadline = Instant::now() + HERDR_WAIT;
        while forks
            .iter()
            .any(|(session, process)| process.is_running() || self.transcript(session).exists())
        {
            assert!(Instant::now() < deadline, "a fork outlived its question");
            thread::sleep(Duration::from_millis(25));
        }
    }

    /// The sessions and processes of the saved forks.
    fn processes(&self) -> Vec<(String, ProcessStamp)> {
        self.saved()
            .forks
            .iter()
            .map(|fork| (fork.session.clone(), fork.process.expect("a started fork")))
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

/// The next question, as the fork told the prompt `prompt` submits it.
fn fork_question(flow: &ExploreFlow, prompt: &str) -> serde_json::Value {
    serde_json::json!({
        "instance": prompt_line(prompt, "Explore round: "),
        "request": prompt_line(prompt, "Explore request: "),
        "checkpoint": flow.exploration.comparison.checkpoint,
        "interpretation": {"answer": prompt_line(prompt, "Answer ID: "), "status": "accepted",
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
        let process = fork.process.unwrap();
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
    fs::write(run.forks().join(format!("{}.end", sessions[0])), "").unwrap();
    let deadline = Instant::now() + HERDR_WAIT;
    while run.saved().forks[0].usage.is_none() {
        assert!(Instant::now() < deadline, "the fork's end was not saved");
        thread::sleep(Duration::from_millis(25));
    }
    assert_eq!(run.saved().forks[0].usage.unwrap().cache_read, 1000);

    let processes = run.processes();
    run.flow.turn(
        Some(AnswerInput {
            option: Some("keep".into()),
            text: "Keep it, with a regression test.".into(),
            in_reply_to: None,
            first_pick: None,
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
        fs::write(self.forks().join(format!("{session}.submitted")), "").unwrap();
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
                }),
                question.as_ref(),
            )
            .unwrap();
        self.flow
            .fixture
            .explore(ExploreCommand::Turn(Box::new(request.clone())));
        request
    }

    /// Waits until the saved round has `count` questions, and returns it.
    fn wait_for_questions(&self, count: usize) -> review_explore::ExploreRound {
        let deadline = Instant::now() + HERDR_WAIT;
        loop {
            let saved = self
                .flow
                .fixture
                .runtime
                .store
                .load_explore(
                    &self.flow.fixture.review_unit,
                    &self.flow.exploration.instance,
                )
                .unwrap()
                .unwrap();
            if saved.exploration.questions.len() == count {
                return saved;
            }
            assert!(
                Instant::now() < deadline,
                "the round has {} questions; run-ahead's log:\n{}",
                saved.exploration.questions.len(),
                self.log()
            );
            thread::sleep(Duration::from_millis(25));
        }
    }

    /// Waits until the stand-in in the pane is done with the prompt it read last, which it
    /// works on for a moment after the agent's turn was already submitted, and Herdr reports it
    /// idle.
    fn wait_until_idle(&self) {
        thread::sleep(Duration::from_secs(2));
        let pane = self.flow.fixture.herdr.pane_id.clone();
        let deadline = Instant::now() + HERDR_WAIT;
        while !self
            .flow
            .fixture
            .herdr
            .client()
            .get_agent(&pane)
            .unwrap()
            .is_some_and(|agent| agent.agent_status.waits_for_prompt())
        {
            assert!(Instant::now() < deadline, "the agent stays busy");
            thread::sleep(Duration::from_millis(25));
        }
    }

    /// The sessions the agent in the pane resumed, in order.
    fn resumed(&self) -> Vec<String> {
        fs::read_to_string(resumes_path(&self.root().join("prompt.txt")))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
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
    let prompts = fs::read_to_string(run.root().join("prompt.txt")).unwrap();

    let answered = Instant::now();
    let request = run.answer_bare("keep");

    let saved = run.wait_for_questions(2);
    eprintln!(
        "the prepared question was saved {:?} after the answer",
        answered.elapsed()
    );
    assert_eq!(run.resumed(), std::slice::from_ref(&fork));
    let pane = run.flow.fixture.herdr.pane_id.clone();
    let agent = run
        .flow
        .fixture
        .herdr
        .client()
        .get_agent(&pane)
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
        fs::read_to_string(run.root().join("prompt.txt")).unwrap(),
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
        }),
        3,
    );
    assert!(
        fs::read_to_string(run.transcript(&fork))
            .unwrap()
            .contains("\"uuid\":\"prompt-2\""),
        "the prompt reached the fork's session"
    );
    let deadline = Instant::now() + HERDR_WAIT;
    let third = loop {
        let saved = run.saved();
        if let Some(fork) = saved.forks.get(4)
            && run.forks().join(format!("{}.args", fork.session)).exists()
        {
            break fork.session.clone();
        }
        assert!(Instant::now() < deadline, "{}", run.log());
        thread::sleep(Duration::from_millis(25));
    };
    assert!(
        run.fork_file(&third, "args")
            .lines()
            .collect::<Vec<_>>()
            .windows(2)
            .any(|pair| pair == ["--resume", fork.as_str()]),
        "forks of the next question copy the fork's session"
    );
}

#[test]
fn a_switch_herdr_never_confirms_puts_the_agent_back_and_retry_reaches_its_own_session_once() {
    let mut run = RunAheadFlow::start();
    run.flow.turn(None, 1);
    let sessions = run.wait_for_forks(2);
    let fork = sessions[0].clone();
    run.fork_submits(&fork);
    fs::write(run.root().join("prompt.unreported-resumes"), "").unwrap();
    let prompts = fs::read_to_string(run.root().join("prompt.txt")).unwrap();
    run.wait_until_idle();

    let request = run.answer_bare("keep");

    // The switch waits for Herdr in vain, then the agent goes back to its own session.
    let deadline = Instant::now() + Duration::from_secs(60);
    while !matches!(
        run.saved().forks[0].continued,
        Some(Continuation::Undone { .. })
    ) {
        assert!(Instant::now() < deadline, "{}", run.log());
        thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(run.resumed(), [fork.clone(), "session".to_owned()]);
    assert_eq!(
        fs::read_to_string(run.root().join("prompt.txt")).unwrap(),
        prompts,
        "the agent in the pane got no prompt yet"
    );
    run.wait_until_gone(&run.processes()[..1]);
    let saved = run.wait_for_questions(1);
    assert_eq!(saved.exploration.conversation.len(), 1);

    fs::remove_file(run.root().join("prompt.unreported-resumes")).unwrap();
    run.flow
        .fixture
        .explore(ExploreCommand::Retry(Box::new(request.clone())));

    let deadline = Instant::now() + HERDR_WAIT;
    let delivered = loop {
        let delivered = fs::read_to_string(run.root().join("prompt.txt")).unwrap();
        if delivered.len() > prompts.len() {
            break delivered[prompts.len()..].to_owned();
        }
        assert!(Instant::now() < deadline, "{}", run.log());
        thread::sleep(Duration::from_millis(25));
    };
    assert!(
        delivered.contains(&format!("Explore request: {}\n", request.request)),
        "{delivered}"
    );
    assert!(
        fs::read_to_string(run.transcript("session"))
            .unwrap()
            .contains("\"uuid\":\"prompt-2\""),
        "the answer reached the agent's own session"
    );
    thread::sleep(Duration::from_millis(500));
    assert_eq!(
        fs::read_to_string(run.root().join("prompt.txt"))
            .unwrap()
            .matches(&format!("Explore request: {}\n", request.request))
            .count(),
        1,
        "the answer reached the agent once"
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
        assert!(
            !herdr
                .server
                .root()
                .join("claude-config/projects/standin")
                .join(format!("{session}.jsonl"))
                .exists()
        );
    }
}

/// The agent's turns of the real test: Claude Code on the owner's subscription, at the model
/// the run-ahead trials use.
const REAL_MODEL: &str = "claude-sonnet-5-5";
/// How long a real turn may take.
const REAL_TURN: Duration = Duration::from_secs(900);

/// The change of the real test: one that holds a decision, so the agent asks about it.
const REAL_CHANGE: &[u8] =
    b"/// What a resolved review conversation does when the code it is attached to changes.\n\
      pub enum Reopen {\n    Never,\n    WhenItsLinesChange,\n    WhenItsFileChanges,\n}\n\n\
      /// The policy every review applies; there is no setting for it.\n\
      pub const POLICY: Reopen = Reopen::WhenItsFileChanges;\n\n\
      /// Whether a resolved conversation reopens under `policy`.\n\
      pub fn reopens(policy: &Reopen, lines_changed: bool, file_changed: bool) -> bool {\n    \
      match policy {\n        Reopen::Never => false,\n        \
      Reopen::WhenItsLinesChange => lines_changed,\n        \
      Reopen::WhenItsFileChanges => file_changed,\n    }\n}\n";

/// A round whose agent is real Claude Code: the stand-in in the pane reports the session that
/// a headless `claude` takes the agent's turns in, and its forks run `claude` too. Dropping it
/// stops the reviewer, then deletes the test's project in the owner's Claude Code
/// configuration, named after the test's temporary repository.
struct RealRun {
    run: Option<RunAheadFlow>,
    /// The agent's session.
    session: String,
    /// The agent's flags, which its forks keep.
    flags: Vec<String>,
    /// When the stand-in in the pane started.
    started: Instant,
}

impl RealRun {
    fn start() -> Self {
        let session = uuid::Uuid::new_v4().to_string();
        let repository_files = repository_fixture(RepoType::Git);
        repository_files.write("reviewed.rs", REAL_CHANGE);
        let started = Instant::now();
        let herdr = IsolatedHerdrServer::start_forkable(
            repository_files.root(),
            &session,
            StandIn::ForkedForReal,
        );
        herdr.show_idle();
        let root = herdr.server.root().to_owned();
        let fixture = ReviewFlowFixture::start_on(repository_files, herdr);
        fixture
            .runtime
            .store
            .save_explore_run_ahead(RunAhead::Every)
            .unwrap();
        // The reviewer's MCP server only, and the unreviewed diffs' temporary directory.
        let mcp = serde_json::json!({"mcpServers": {"herdr_reviewer": {
            "command": super::effects::fixture::test_fork_tools().control.with_file_name("reviewer-mcp"),
            "args": [fixture.endpoint.address().port().to_string()],
        }}})
        .to_string();
        let flags: Vec<String> = [
            "--strict-mcp-config",
            "--mcp-config",
            &mcp,
            "--permission-mode",
            "dontAsk",
            "--add-dir",
            &std::env::temp_dir().to_string_lossy(),
        ]
        .map(str::to_owned)
        .to_vec();
        fs::write(
            root.join("real-claude"),
            format!("claude\n{}", flags.join("\n")),
        )
        .unwrap();
        Self {
            run: Some(RunAheadFlow {
                flow: ExploreFlow::start_on(fixture),
            }),
            session,
            flags,
            started,
        }
    }

    /// Draws the stand-in's empty input box again once the test harness that runs it warned,
    /// on the pane's screen, that it has run for over 60 seconds: that line is no text of the
    /// reviewer's.
    fn clear_harness_warning(&mut self) {
        let warned = self.started + Duration::from_secs(61);
        thread::sleep(warned.saturating_duration_since(Instant::now()));
        let run = self.run();
        fs::write(run.root().join("prompt.screen"), "❯ ").unwrap();
        let pane = run.flow.fixture.herdr.pane_id.clone();
        let deadline = Instant::now() + HERDR_WAIT;
        while run
            .flow
            .fixture
            .herdr
            .client()
            .read_agent_screen_styled(&pane)
            .is_ok_and(|screen| screen.trim() != "❯")
        {
            assert!(
                Instant::now() < deadline,
                "the stand-in did not draw its screen"
            );
            thread::sleep(Duration::from_millis(25));
        }
    }

    fn run(&mut self) -> &mut RunAheadFlow {
        self.run.as_mut().unwrap()
    }

    /// The kickoff, taken by the agent: the round's first question.
    fn kickoff(&mut self) -> review_explore::Question {
        let flow = &mut self.run().flow;
        let kickoff = flow.exploration.request(None, None).unwrap();
        flow.fixture
            .explore(ExploreCommand::Turn(Box::new(kickoff.clone())));
        flow.wait_for_prompt(&kickoff);
        let prompt =
            fs::read_to_string(flow.fixture.herdr.server.root().join("prompt.txt")).unwrap();
        self.agent_turn(&prompt, None);
        self.run()
            .flow
            .exploration
            .questions
            .last()
            .cloned()
            .expect("the agent asked a question")
    }

    /// The agent takes its turn on `prompt`, in the session the pane reports: its own, or the
    /// session `resumed` once it resumed one, while the test acknowledges each saved turn as
    /// the pane does.
    fn agent_turn(&mut self, prompt: &str, resumed: Option<&str>) {
        let repository = self.run().flow.fixture.runtime.repository.root().to_owned();
        // The agent in the pane works while the headless agent takes its turn, as Claude Code
        // in the pane would: forks are taken once it is idle again.
        let root = self.run().root().to_owned();
        fs::write(root.join("prompt.state"), "⠋ Working").unwrap();
        let session = match resumed {
            Some(session) => ["--resume", session],
            None => ["--session-id", &self.session],
        };
        let mut agent = Command::new("claude")
            .arg("-p")
            .args(session)
            .args(["--model", REAL_MODEL])
            .args(&self.flags)
            .arg("--allowedTools")
            .args(claude_fork::SUBMITS)
            .current_dir(repository)
            // Never the Herdr this test runs in: the agent's hooks would report to it.
            .env_clear()
            .envs(
                std::env::vars_os()
                    .filter(|(name, _)| !name.as_encoded_bytes().starts_with(b"HERDR_")),
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        agent
            .stdin
            .take()
            .unwrap()
            .write_all(prompt.as_bytes())
            .unwrap();
        let flow = &mut self.run().flow;
        let deadline = Instant::now() + REAL_TURN;
        while agent.try_wait().unwrap().is_none() {
            assert!(Instant::now() < deadline, "the agent's turn did not end");
            if let Some(event) = flow
                .fixture
                .runtime
                .recv_timeout(Duration::from_millis(100))
                && let Some(committed) = event.downcast_ref::<ui_events::ExploreCommitted>()
            {
                flow.exploration = committed.round.exploration.clone();
                let _ = committed.response.send(Ok(committed.applied));
            }
        }
        flow.fixture.herdr.show_idle();
    }

    /// Waits until `count` forks ran to their end.
    fn wait_for_forks_to_end(&mut self, count: usize) -> RoundForks {
        let run = self.run();
        let deadline = Instant::now() + REAL_TURN;
        loop {
            let saved = run.saved();
            if saved.forks.len() == count && saved.forks.iter().all(|fork| fork.exit.is_some()) {
                eprintln!("run-ahead's log:\n{}", run.log());
                return saved;
            }
            assert!(
                Instant::now() < deadline,
                "the forks did not end: {}",
                run.log()
            );
            thread::sleep(Duration::from_secs(1));
        }
    }

    /// The transcripts of the owner's Claude Code configuration.
    fn projects() -> claude_fork::Transcripts {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap());
        claude_fork::Transcripts::at(home.join(".claude/projects"))
    }
}

impl Drop for RealRun {
    fn drop(&mut self) {
        drop(self.run.take());
        if let Some(own) = Self::projects().find(&self.session) {
            let _ = fs::remove_dir_all(own.parent().unwrap());
        }
    }
}

#[test]
#[ignore = "real Claude Code turns on the owner's subscription (claude-sonnet-5-5); run with --ignored"]
fn a_real_claude_code_fork_takes_its_answer_s_turn_and_the_reviewer_keeps_it() {
    let mut real = RealRun::start();
    let question = real.kickoff();

    // One real fork per choice takes its turn and submits it with its own access.
    let saved = real.wait_for_forks_to_end(question.alternatives.len());

    let run = real.run();
    let kept: Vec<_> = saved
        .forks
        .iter()
        .filter_map(|fork| fork.turn.as_ref())
        .collect();
    assert!(!kept.is_empty(), "no fork's turn was kept: {}", run.log());
    let requests: std::collections::HashSet<_> = kept.iter().map(|turn| &turn.request).collect();
    assert_eq!(
        requests.len(),
        kept.len(),
        "each fork submits its own request"
    );
    assert!(
        kept.iter()
            .all(|turn| turn.instance == run.flow.exploration.instance)
    );
    assert!(
        saved
            .forks
            .iter()
            .all(|fork| fork.usage.is_some_and(|usage| usage.output > 0))
    );
    assert_eq!(run.flow.exploration.questions.len(), question_count(run));

    // A Reset stops the forks and deletes their transcripts; the agent's own stays.
    let processes = run.processes();
    run.flow.fixture.explore(ExploreCommand::Reset);
    let projects = RealRun::projects();
    let deadline = Instant::now() + HERDR_WAIT;
    while processes
        .iter()
        .any(|(fork, process)| process.is_running() || projects.find(fork).is_some())
    {
        assert!(Instant::now() < deadline, "a fork outlived its round");
        thread::sleep(Duration::from_millis(100));
    }
    assert!(
        projects.find(&real.session).is_some(),
        "the agent's own transcript stays"
    );
}

/// How many questions the saved round has: forks never add one.
fn question_count(run: &RunAheadFlow) -> usize {
    run.flow
        .fixture
        .runtime
        .store
        .load_explore(
            &run.flow.fixture.review_unit,
            &run.flow.exploration.instance,
        )
        .unwrap()
        .unwrap()
        .exploration
        .questions
        .len()
}

#[test]
#[ignore = "real Claude Code turns on the owner's subscription (claude-sonnet-5-5); run with --ignored"]
fn a_real_claude_code_agent_continues_as_the_fork_of_a_bare_answer() {
    let mut real = RealRun::start();
    let question = real.kickoff();
    let saved = real.wait_for_forks_to_end(question.alternatives.len());
    // A fork that asked a question, when one did, so that the agent takes a next turn.
    let fork = saved
        .forks
        .iter()
        .find(|fork| fork.turn.as_ref().is_some_and(|turn| turn.next.is_some()))
        .or_else(|| saved.forks.iter().find(|fork| fork.turn.is_some()))
        .expect("a fork's turn was kept")
        .clone();

    real.clear_harness_warning();
    let run = real.run();
    // No forks for the next questions: their real turns would cost and prove nothing more here.
    run.flow
        .fixture
        .runtime
        .store
        .save_explore_run_ahead(RunAhead::Off)
        .unwrap();
    let answered = Instant::now();
    let request = run.answer_bare(&fork.choice);
    let round = run.wait_for_questions_or_conclusion(2);
    eprintln!(
        "the prepared turn was saved {:?} after the answer",
        answered.elapsed()
    );

    assert_eq!(run.resumed(), std::slice::from_ref(&fork.session));
    let pane = run.flow.fixture.herdr.pane_id.clone();
    let agent = run
        .flow
        .fixture
        .herdr
        .client()
        .get_agent(&pane)
        .unwrap()
        .unwrap();
    assert_eq!(
        agent.agent_session.as_ref().map(|session| &session.value),
        Some(&fork.session)
    );
    let turn = &round.exploration.conversation.last().unwrap().update;
    assert_eq!(turn.request, request.request);
    assert!(
        RealRun::projects().find(&fork.session).is_some(),
        "the fork's session, now the agent's, keeps its transcript"
    );
    if turn.conclusion.is_some() {
        eprintln!("the fork concluded the round: no next answer to send");
        return;
    }

    // The next answer, with a comment, goes to the agent, which resumes the fork's session and
    // takes its turn from there.
    run.flow.exploration = round.exploration.clone();
    let prompts = run.root().join("prompt.txt");
    let offset = fs::read_to_string(&prompts).unwrap().len();
    let question = run.flow.exploration.questions.last().cloned().unwrap();
    let next = run
        .flow
        .exploration
        .request(
            Some(AnswerInput {
                option: Some(question.alternatives[0].id.clone()),
                text: "Yes, and write down why.".into(),
                in_reply_to: None,
                first_pick: None,
            }),
            Some(&question),
        )
        .unwrap();
    run.flow
        .fixture
        .explore(ExploreCommand::Turn(Box::new(next.clone())));
    run.flow.wait_for_prompt(&next);
    let prompt = fs::read_to_string(&prompts).unwrap()[offset..].to_owned();
    real.agent_turn(&prompt, Some(&fork.session));

    let run = real.run();
    let round = run.wait_for_questions_or_conclusion(3);
    eprintln!("run-ahead's log:\n{}", run.log());
    assert_eq!(
        round
            .exploration
            .conversation
            .last()
            .unwrap()
            .update
            .request,
        next.request,
        "the agent's turn on the fork's session is accepted"
    );
    assert_eq!(
        round.last_agent_session,
        review_explore::ConversationBinding::from_agent(&agent)
    );
}

impl RunAheadFlow {
    /// Waits until the saved round has `count` turns of the agent, and returns it.
    fn wait_for_questions_or_conclusion(&self, count: usize) -> review_explore::ExploreRound {
        let deadline = Instant::now() + REAL_TURN;
        loop {
            let saved = self
                .flow
                .fixture
                .runtime
                .store
                .load_explore(
                    &self.flow.fixture.review_unit,
                    &self.flow.exploration.instance,
                )
                .unwrap()
                .unwrap();
            if saved.exploration.conversation.len() >= count {
                return saved;
            }
            assert!(Instant::now() < deadline, "{}", self.log());
            thread::sleep(Duration::from_millis(100));
        }
    }
}
