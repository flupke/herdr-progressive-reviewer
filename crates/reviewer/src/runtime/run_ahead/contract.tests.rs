//! The run-ahead contract: what the stand-in assumes of Claude Code, checked against the real
//! one. Each body runs twice. Its rehearsal runs the stand-in in the agent's pane, in `make
//! check`: it finds the faults of the test itself, its assertions, timing and input box, but
//! for a timing fault that only real Claude Code's pace brings out. Its
//! contract check, ignored by default, runs real Claude Code there on the token's account,
//! with a scratch configuration: it tells whether the real agent still behaves as the stand-in
//! assumes. A body says which [`Assumption`] each of its steps rests on, and a failure names it.

use std::ffi::OsString;
use std::process::Command;

use review_explore::{AnswerInput, ExploreRound, Question, TurnRequest};
use review_run_ahead::ForkRecord;

use super::*;

/// The model of the real agent and its forks: the one the run-ahead trials use.
const REAL_MODEL: &str = "claude-sonnet-5-5";
/// How long a step may wait without news when the agent is real: a real turn.
const REAL_TURN: Duration = Duration::from_secs(900);

/// The change of the contract check: one that holds a decision, so that the real agent asks
/// about it.
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

/// What the stand-in assumes of Claude Code, on which a step of a body rests.
#[derive(Clone, Copy, Debug)]
enum Assumption {
    /// Claude Code starts in the agent's pane, logged in, runs Herdr's and the reviewer's hooks,
    /// and takes an Explore turn through the reviewer's tools.
    Starts,
    /// A fork that run-ahead starts with the agent's command line, as it builds it, takes its
    /// turn and submits it with its own access.
    ForkCommandLine,
    /// The agent moves to the session that a typed `/resume` names, and Herdr reports it there
    /// once Claude Code's session hook ran.
    ResumeFollowed,
    /// Claude Code takes what is typed in its pane in order: what is typed after a `/resume`
    /// reaches the session that `/resume` moved the agent to.
    TypedInOrder,
    /// Claude Code keeps a session's transcript where run-ahead finds it, and deletes it, under
    /// `projects` in its configuration directory.
    Transcripts,
}

impl Assumption {
    fn says(self) -> &'static str {
        match self {
            Self::Starts => {
                "Claude Code starts in the pane, logged in, with Herdr's and the reviewer's \
                 hooks, and takes an Explore turn through the reviewer's tools"
            }
            Self::ForkCommandLine => {
                "a fork started with the command line run-ahead builds takes its turn and \
                 submits it with its own access"
            }
            Self::ResumeFollowed => {
                "a typed /resume moves the agent to the session, and Herdr reports it there"
            }
            Self::TypedInOrder => {
                "Claude Code takes what is typed in its pane in order, so what follows a \
                 /resume reaches that session"
            }
            Self::Transcripts => {
                "Claude Code keeps each session's transcript under projects/ in its \
                 configuration directory, where run-ahead deletes a fork's"
            }
        }
    }
}

/// The agent in the pane of a contract body.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Agent {
    /// The stand-in, whose turns and whose forks' turns the test plays.
    StandIn,
    /// Real Claude Code, which takes its turns and its forks' turns itself.
    Real,
}

/// A round with run-ahead on for every choice, whose agent is `agent`, and the assumption the
/// step under way rests on, which a failure names.
struct ContractRun {
    run: RunAheadFlow,
    agent: Agent,
    /// The agent's own session.
    session: String,
    resting_on: Assumption,
}

impl ContractRun {
    /// The round with the agent `agent`, whose forks' host waits as `waits` say.
    fn start(agent: Agent, waits: claude_fork::ForkWaits) -> Self {
        let (run, session) = match agent {
            Agent::StandIn => (
                RunAheadFlow::start_with(StandIn::Forkable, waits),
                "session".into(),
            ),
            Agent::Real => {
                let session = uuid::Uuid::new_v4().to_string();
                let start = std::panic::AssertUnwindSafe(|| real_run(&session, waits));
                let run = std::panic::catch_unwind(start).unwrap_or_else(|panic| {
                    eprintln!("{}", failed(Agent::Real, Assumption::Starts));
                    std::panic::resume_unwind(panic)
                });
                (run, session)
            }
        };
        Self {
            run,
            agent,
            session,
            resting_on: Assumption::Starts,
        }
    }

    /// The steps from here on rest on `assumption`.
    fn rests_on(&mut self, assumption: Assumption) {
        self.resting_on = assumption;
    }

    /// How long a step waits without news.
    fn patience(&self) -> Duration {
        match self.agent {
            Agent::StandIn => GUARD,
            Agent::Real => REAL_TURN,
        }
    }

    fn herdr(&self) -> &IsolatedHerdrServer {
        self.run.herdr()
    }

    /// The kickoff, and the question the agent asks on its turn.
    fn kickoff(&mut self) -> Question {
        let flow = &mut self.run.flow;
        let kickoff = flow.exploration.request(None, None).unwrap();
        let kickoff = flow.post_turn(&kickoff);
        flow.wait_for_prompt(&kickoff);
        self.agent_turn(&kickoff, 1);
        self.run
            .flow
            .exploration
            .questions
            .last()
            .cloned()
            .expect("the agent asked a question")
    }

    /// The agent takes its turn `version` on the prompt of `request`, which it read: the
    /// stand-in submits a question as the test says, and real Claude Code as it sees fit, while
    /// the test acknowledges each saved turn as the pane does. Returns once Herdr reports the
    /// agent idle again.
    fn agent_turn(&mut self, request: &TurnRequest, version: u32) {
        let patience = self.patience();
        let statuses = self.herdr().server.agent_statuses(&self.herdr().pane_id);
        // Herdr may still report the agent idle as it reads the prompt.
        statuses.wait_within(patience, herdr_client::protocol::AgentStatus::Working);
        let flow = &mut self.run.flow;
        match self.agent {
            Agent::StandIn => {
                // The stand-in holds its turn until it submitted, as an agent does that calls
                // the reviewer's tools during its turn.
                let accepted =
                    flow.submit(&super::super::explore_flow::agent_update(request, version));
                assert_ne!(accepted.is_error, Some(true), "{accepted:?}");
                flow.fixture
                    .herdr
                    .events()
                    .send(&StandInRole::Agent, &StandInCommand::EndTurn);
            }
            Agent::Real => loop {
                let event = flow
                    .fixture
                    .runtime
                    .recv_timeout(patience)
                    .expect("the agent's turn did not end");
                if let Some(committed) = event.downcast_ref::<ui_events::ExploreCommitted>() {
                    flow.exploration = committed.round.exploration.clone();
                    let _ = committed.response.send(Ok(committed.applied));
                    if committed
                        .round
                        .exploration
                        .conversation
                        .iter()
                        .any(|entry| entry.update.request == request.request)
                    {
                        break;
                    }
                }
            },
        }
        statuses.wait_within(patience, herdr_client::protocol::AgentStatus::Idle);
    }

    /// The forks of each of the `count` choices of the question take their turn, submit it
    /// and end; returns the saved forks once they all ended.
    fn forks_take_turns(&mut self, count: usize) -> RoundForks {
        if self.agent == Agent::StandIn {
            for session in self.run.wait_for_forks(count) {
                self.run.fork_submits(&session);
                self.herdr()
                    .events()
                    .send(&StandInRole::Fork { session }, &StandInCommand::End);
            }
        }
        let saved = self.wait_for_saved("the forks' end", |saved| {
            (saved.forks.len() == count && saved.forks.iter().all(|fork| fork.exit.is_some()))
                .then(|| saved.clone())
        });
        eprintln!("run-ahead's log:\n{}", self.run.log());
        saved
    }

    /// A fork that prepared a turn, one that asked a question when one did, so that the agent
    /// takes a next turn.
    fn fork_with_a_turn(saved: &RoundForks) -> ForkRecord {
        saved
            .forks
            .iter()
            .find(|fork| fork.turn.as_ref().is_some_and(|turn| turn.next.is_some()))
            .or_else(|| saved.forks.iter().find(|fork| fork.turn.is_some()))
            .expect("a fork prepared a turn")
            .clone()
    }

    /// What `check` finds in the saved forks, once it finds something.
    fn wait_for_saved<T>(&self, what: &str, check: impl Fn(&RoundForks) -> Option<T>) -> T {
        self.run.flow.state.wait_within(
            self.patience(),
            what,
            || check(&self.run.saved()),
            || format!("run-ahead's log:\n{}", self.run.log()),
        )
    }

    /// Waits until the saved round has `count` turns of the agent, and returns it.
    fn wait_for_turns(&self, count: usize) -> ExploreRound {
        let flow = &self.run.flow;
        flow.state.wait_within(
            self.patience(),
            &format!("{count} turns"),
            || {
                let saved = flow.saved();
                (saved.exploration.conversation.len() >= count).then_some(saved)
            },
            || format!("run-ahead's log:\n{}", self.run.log()),
        )
    }

    /// The sessions the agent resumed, once it resumed `count`.
    fn resumed(&self, count: usize) -> Vec<String> {
        self.herdr()
            .events()
            .wait_until(&format!("{count} resumed sessions"), |received| {
                resumed_sessions(received).len() >= count
            });
        self.herdr().resumed()
    }

    /// The session Herdr reports the agent in the pane on.
    fn reported_session(&self) -> Option<String> {
        self.herdr()
            .client()
            .get_agent(&self.herdr().pane_id)
            .unwrap()
            .and_then(|agent| agent.agent_session)
            .map(|session| session.value)
    }

    /// The transcripts of the agent's Claude Code configuration, the scratch one of the test.
    fn transcripts(&self) -> claude_fork::Transcripts {
        claude_fork::Transcripts::at(self.run.root().join(CLAUDE_CONFIG).join("projects"))
    }

    /// Whether the prompt of `request` reached the agent on the session `session`: its
    /// transcript holds the prompt, which names the turn's own directory.
    fn reached(&self, session: &str, request: &TurnRequest) -> bool {
        let explore_request = format!("Explore request: {}\n", request.request);
        let prompts = self.herdr().prompts();
        let prompt = prompts
            .iter()
            .find(|prompt| prompt.contains(&explore_request))
            .expect("the agent read the prompt");
        let index = prompt
            .split_whitespace()
            .find(|word| word.ends_with("/index.md"))
            .expect("the prompt names its turn's index");
        let path = self
            .transcripts()
            .find(session)
            .unwrap_or_else(|| panic!("no transcript of the session {session}"));
        fs::read_to_string(path).unwrap().contains(index)
    }
}

impl Drop for ContractRun {
    fn drop(&mut self) {
        if !thread::panicking() {
            return;
        }
        eprintln!("{}", failed(self.agent, self.resting_on));
        if self.agent == Agent::Real {
            eprintln!("Its pane shows:\n{}", self.herdr().screen());
        }
    }
}

/// What a failure of `agent` at a step that rests on `assumption` means.
fn failed(agent: Agent, assumption: Assumption) -> String {
    let assumption = assumption.says();
    match agent {
        Agent::StandIn => format!(
            "\nThe rehearsal failed where the test checks that {assumption}: the stand-in plays \
             it, so the fault is the test's."
        ),
        Agent::Real => format!(
            "\nThe contract check failed where the test checks that {assumption}: real Claude \
             Code may no longer behave as the stand-in assumes."
        ),
    }
}

/// The forks of a question take their turns; the reviewer answers with the choice of one that
/// prepared its turn and no comment, so the agent continues as that fork; the next answer, with a
/// comment, reaches the fork's session, where the agent takes its turn.
fn continue_as_the_fork_of_a_bare_answer(mut contract: ContractRun) {
    let question = contract.kickoff();

    contract.rests_on(Assumption::ForkCommandLine);
    let saved = contract.forks_take_turns(question.alternatives.len());
    contract.check_prepared_turns(&saved);
    let fork = ContractRun::fork_with_a_turn(&saved);

    contract.rests_on(Assumption::ResumeFollowed);
    contract.stop_running_ahead();
    let request = contract.run.answer_bare(&fork.choice);
    let round = contract.wait_for_turns(2);
    let turn = &round.exploration.conversation.last().unwrap().update;
    assert_eq!(
        turn.request, request.request,
        "the fork's turn is the round's"
    );
    assert_eq!(contract.resumed(1), std::slice::from_ref(&fork.session));
    assert_eq!(
        contract.reported_session().as_ref(),
        Some(&fork.session),
        "Herdr reports the agent on the fork's session"
    );

    contract.rests_on(Assumption::Transcripts);
    contract.check_only_the_fork_s_transcript_stays(&fork);
    if turn.conclusion.is_some() {
        eprintln!("the fork concluded the round: no next answer to send");
        return;
    }

    contract.rests_on(Assumption::TypedInOrder);
    contract.run.flow.exploration = round.exploration.clone();
    let next = contract.answer_with_a_comment();
    assert!(
        contract.reached(&fork.session, &next),
        "the answer reached the fork's session"
    );
    let round = contract.wait_for_turns(3);
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
    let agent = contract
        .herdr()
        .client()
        .get_agent(&contract.herdr().pane_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        round.last_agent_session,
        review_explore::ConversationBinding::from_agent(&agent)
    );
}

impl ContractRun {
    /// Checks that each fork of `saved` that prepared a turn submitted its own, in the round, and
    /// said what it spent, and that no fork added a question to the round.
    fn check_prepared_turns(&self, saved: &RoundForks) {
        let prepared: Vec<_> = saved
            .forks
            .iter()
            .filter_map(|fork| fork.turn.as_ref())
            .collect();
        assert!(!prepared.is_empty(), "no fork prepared a turn");
        let requests: std::collections::HashSet<_> =
            prepared.iter().map(|turn| &turn.request).collect();
        assert_eq!(
            requests.len(),
            prepared.len(),
            "each fork submits its own request"
        );
        let instance = &self.run.flow.exploration.instance;
        assert!(prepared.iter().all(|turn| turn.instance == *instance));
        assert!(
            saved
                .forks
                .iter()
                .all(|fork| fork.usage.is_some_and(|usage| usage.output > 0)),
            "each fork's output says what it spent"
        );
        assert_eq!(
            self.run.flow.saved().exploration.questions.len(),
            1,
            "a fork adds no question to the round"
        );
    }

    /// No forks for the next questions: they would cost and prove nothing more.
    fn stop_running_ahead(&self) {
        self.run
            .flow
            .fixture
            .runtime
            .store
            .save_explore_run_ahead(RunAhead::Off)
            .unwrap();
    }

    /// Waits until run-ahead stopped every fork but `fork`, whose session the agent runs, and
    /// deleted their transcripts; checks that `fork`'s stays.
    fn check_only_the_fork_s_transcript_stays(&self, fork: &ForkRecord) {
        self.wait_for_saved("the other forks' clean-up", |saved| {
            saved
                .forks
                .iter()
                .filter(|other| other.session != fork.session)
                .all(|other| other.cleaned)
                .then_some(())
        });
        for other in self.run.saved().forks {
            if other.session == fork.session {
                assert!(!other.cleaned, "the fork's session is the agent's now");
                assert!(self.transcripts().find(&fork.session).is_some());
            } else {
                assert!(
                    !other.process.unwrap().is_running(),
                    "a fork outlived its question"
                );
                assert_eq!(self.transcripts().find(&other.session), None);
            }
        }
    }

    /// Answers the latest question with its first choice and a comment, and returns once the
    /// agent took its turn on it.
    fn answer_with_a_comment(&mut self) -> TurnRequest {
        let flow = &mut self.run.flow;
        let question = flow.exploration.questions.last().cloned().unwrap();
        let answer = AnswerInput {
            option: Some(question.alternatives[0].id.clone()),
            text: "Yes, and write down why.".into(),
            in_reply_to: None,
            first_pick: None,
            ..AnswerInput::default()
        };
        let request = flow
            .exploration
            .request(Some(answer), Some(&question))
            .unwrap();
        let request = flow.post_turn(&request);
        flow.wait_for_prompt(&request);
        self.agent_turn(&request, 3);
        request
    }
}

/// A switch that nobody confirms in time, since it waits no time: the agent resumes the fork's
/// session, then, typed right after, its own, where it ends; the answer, retried, reaches its
/// own session once.
fn settle_back_on_the_agent_s_own_session(mut contract: ContractRun) {
    let question = contract.kickoff();

    contract.rests_on(Assumption::ForkCommandLine);
    let saved = contract.forks_take_turns(question.alternatives.len());
    let fork = ContractRun::fork_with_a_turn(&saved);

    contract.rests_on(Assumption::ResumeFollowed);
    if contract.agent == Agent::StandIn {
        // Real Claude Code's hook reports the resume about 0.5 s after it is typed, after a
        // switch that waits no time gave up; the stand-in's could come first.
        fs::write(contract.herdr().directory().unreported_resume(), "").unwrap();
    }
    contract.stop_running_ahead();
    let request = contract.run.answer_bare(&fork.choice);
    contract.wait_for_saved("the switch undone", |saved| {
        let continued = &saved.fork(&fork.session)?.continued;
        matches!(continued, Some(Continuation::Undone { .. })).then_some(())
    });
    let resumed = contract.resumed(2);

    contract.rests_on(Assumption::TypedInOrder);
    assert_eq!(resumed, [fork.session.clone(), contract.session.clone()]);
    assert_eq!(
        contract.reported_session().as_ref(),
        Some(&contract.session),
        "Herdr reports the agent back on its own session"
    );
    assert_eq!(contract.run.flow.saved().exploration.conversation.len(), 1);

    contract
        .run
        .flow
        .fixture
        .explore(ExploreCommand::Retry(Box::new(request.clone())));
    contract.run.flow.wait_for_prompt(&request);
    contract.agent_turn(&request, 2);
    assert!(
        contract.reached(&contract.session, &request),
        "the answer reached the agent's own session"
    );
    let explore_request = format!("Explore request: {}", request.request);
    assert_eq!(
        contract
            .herdr()
            .prompts()
            .iter()
            .filter(|prompt| prompt.contains(&explore_request))
            .count(),
        1,
        "the answer reached the agent once"
    );
}

#[test]
fn rehearse_continuing_as_the_fork_of_a_bare_answer() {
    continue_as_the_fork_of_a_bare_answer(ContractRun::start(Agent::StandIn, test_waits()));
}

#[test]
#[ignore = "the run-ahead contract check: real Claude Code turns on the token's account; see .agents/wiki/run-ahead.md"]
fn real_claude_code_continues_as_the_fork_of_a_bare_answer() {
    continue_as_the_fork_of_a_bare_answer(ContractRun::start(Agent::Real, test_waits()));
}

/// The waits of a switch that nobody confirms in time; the move back to the agent's own
/// session waits as long as in production.
fn unconfirmed_switch() -> claude_fork::ForkWaits {
    claude_fork::ForkWaits {
        resume: Duration::ZERO,
        ..test_waits()
    }
}

#[test]
fn rehearse_settling_back_on_the_agent_s_own_session() {
    settle_back_on_the_agent_s_own_session(ContractRun::start(
        Agent::StandIn,
        unconfirmed_switch(),
    ));
}

#[test]
#[ignore = "the run-ahead contract check: real Claude Code turns on the token's account; see .agents/wiki/run-ahead.md"]
fn real_claude_code_settles_back_on_its_own_session() {
    settle_back_on_the_agent_s_own_session(ContractRun::start(Agent::Real, unconfirmed_switch()));
}

/// The variable that logs the real agent in: a token of `claude setup-token`.
const TOKEN_VARIABLE: &str = "CLAUDE_CODE_OAUTH_TOKEN";

/// A round whose agent is real Claude Code on the session `session`, with a configuration of
/// its own under `claude-config` in the server's directory, gone with the server: the owner's
/// is never read nor written.
fn real_run(session: &str, waits: claude_fork::ForkWaits) -> RunAheadFlow {
    assert!(
        std::env::var_os(TOKEN_VARIABLE).is_some(),
        "set {TOKEN_VARIABLE} to a token of `claude setup-token`: the contract check logs in \
         with it, and never with the owner's Claude Code configuration"
    );
    let repository_files = repository_fixture(RepoType::Git);
    repository_files.write("reviewed.rs", REAL_CHANGE);
    let repository = repository_files.root().to_owned();
    let mut herdr = IsolatedHerdrServer::create(&repository, "claude", |root| {
        real_variables(root)
            .into_iter()
            .map(|(name, value)| format!("{name}={}", value.to_string_lossy()))
            .collect()
    });
    herdr.real_agent = true;
    let configuration = RealConfiguration::write(&herdr, &repository);
    let fixture = ReviewFlowFixture::start_on(repository_files, herdr, |setup| {
        setup.run_ahead.waits = waits;
    });
    configuration.serve_reviewer(fixture.endpoint.address().port());
    let herdr = &fixture.herdr;
    // The forks take the agent's environment.
    let mut command = vec!["pane", "run", &herdr.pane_id.0, "env"];
    let owner_s = owner_s_claude_variables();
    for name in &owner_s {
        command.extend(["-u", name]);
    }
    command.extend(["claude", "--session-id", session, "--model", REAL_MODEL]);
    herdr.run_cli(&command);
    herdr.events().wait_for("the agent's session", |reported| {
        reported.event
            == StandInEvent::SessionReported {
                session: session.into(),
                start: "startup".into(),
            }
    });
    // Herdr hears of the session from its own hook, and sends no event once it does.
    let deadline = std::time::Instant::now() + GUARD;
    loop {
        let agent = herdr.client().get_agent(&herdr.pane_id).unwrap();
        if agent.as_ref().is_some_and(|agent| {
            agent.agent_status.waits_for_prompt()
                && agent
                    .agent_session
                    .as_ref()
                    .is_some_and(|reported| reported.value == session)
        }) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Herdr does not report the agent idle on its session: {agent:?}; its pane shows:\n{}",
            herdr.screen()
        );
        thread::sleep(Duration::from_millis(100));
    }
    fixture
        .runtime
        .store
        .save_explore_run_ahead(RunAhead::Every)
        .unwrap();
    RunAheadFlow {
        flow: ExploreFlow::start_on(fixture),
    }
}

/// The variables of the real agent, its forks and the commands that set up its configuration,
/// under the server's directory `root`.
fn real_variables(root: &Path) -> [(&'static str, OsString); 3] {
    [
        ("CLAUDE_CONFIG_DIR", root.join(CLAUDE_CONFIG).into()),
        // Claude Code keeps its MCP servers' logs there, outside its configuration.
        ("XDG_CACHE_HOME", root.join("cache").into()),
        // The agent and its forks run the owner's Claude Code as it is.
        ("DISABLE_AUTOUPDATER", "1".into()),
    ]
}

/// The variables of Claude Code and of its API in the test's environment, which the real agent
/// does not take, but the token and the configuration directory, which the test sets: those of
/// a Claude Code session the check runs under would load its plugins from `~/.claude` and
/// reach that session.
fn owner_s_claude_variables() -> Vec<String> {
    std::env::vars_os()
        .filter_map(|(name, _)| name.into_string().ok())
        .filter(|name| name.starts_with("CLAUDE") || name.starts_with("ANTHROPIC"))
        .filter(|name| name != TOKEN_VARIABLE && name != "CLAUDE_CONFIG_DIR")
        .collect()
}

impl IsolatedHerdrServer {
    /// What the agent's pane shows; read as a test fails, so it never panics.
    fn screen(&self) -> String {
        Command::new(self.server.binary())
            .args(["pane", "read", &self.pane_id.0, "--source", "visible"])
            .env_clear()
            .envs(self.server.environment())
            .output()
            .map_or_else(
                |error| format!("(unread: {error})"),
                |read| String::from_utf8_lossy(&read.stdout).into_owned(),
            )
    }
}

/// The real agent's own Claude Code configuration directory.
struct RealConfiguration(PathBuf);

impl RealConfiguration {
    /// Sets up the configuration of the agent in `herdr`'s pane, which works in `repository`:
    /// Herdr's session hook, the reviewer's plugin with this build's `reviewer-control`, and the
    /// hooks that report what the agent does as the stand-in reports it, each installed as its
    /// owner installs it; the permissions of the tools an Explore turn and its forks use; no
    /// first-run screen.
    fn write(herdr: &IsolatedHerdrServer, repository: &Path) -> Self {
        let root = herdr.server.root().join(CLAUDE_CONFIG);
        fs::create_dir_all(&root).unwrap();
        let configuration = Self(root);
        Self::run(
            herdr,
            herdr.server.binary(),
            &["integration", "install", "claude"],
        );
        let plugin = Path::new(env!("CARGO_MANIFEST_DIR")).join("../claude-hooks");
        Self::run(
            herdr,
            Path::new("claude"),
            &["plugin", "marketplace", "add", &plugin.to_string_lossy()],
        );
        let control = effects::fixture::test_fork_tools().control;
        Self::run(
            herdr,
            Path::new("claude"),
            &[
                "plugin",
                "install",
                "progressive-reviewer@herdr-progressive-reviewer",
                "--config",
                &format!("control={}", control.display()),
            ],
        );
        configuration.edit("settings.json", |settings| {
            let observing = stand_in_agent::observing_hooks(&stand_in_program());
            for (event, hooks) in observing.as_object().unwrap() {
                let entries = settings["hooks"][event].as_array().cloned();
                let mut entries = entries.unwrap_or_default();
                entries.extend(hooks.as_array().unwrap().iter().cloned());
                settings["hooks"][event] = entries.into();
            }
            settings["permissions"] = serde_json::json!({
                "defaultMode": "dontAsk",
                "allow": claude_fork::SUBMITS,
                // The unreviewed diffs' directory.
                "additionalDirectories": [std::env::temp_dir()],
            });
        });
        configuration.edit(".claude.json", |state| {
            state["hasCompletedOnboarding"] = true.into();
            state["theme"] = "dark".into();
            // Claude Code 2.1.310 offers to make auto mode the default at the first prompt
            // typed with "don't ask" on, and its dialog takes that prompt.
            state["hasSeenAutoDefaultNudge"] = true.into();
            state["hasSeenAutoModeEntryWarning"] = true.into();
            for path in [repository.to_owned(), repository.canonicalize().unwrap()] {
                state["projects"][path.to_string_lossy().as_ref()] = serde_json::json!({
                    "hasTrustDialogAccepted": true,
                    "hasCompletedProjectOnboarding": true,
                });
            }
        });
        configuration
    }

    /// Serves the agent the reviewer's MCP server on `port`.
    fn serve_reviewer(&self, port: u16) {
        let bridge = effects::fixture::test_fork_tools()
            .control
            .with_file_name("reviewer-mcp");
        assert!(
            bridge.is_file(),
            "{} is not built: see .agents/wiki/run-ahead.md",
            bridge.display()
        );
        self.edit(".claude.json", |state| {
            state["mcpServers"]["herdr_reviewer"] = serde_json::json!({
                "type": "stdio",
                "command": bridge,
                "args": [port.to_string()],
            });
        });
    }

    /// Runs `program` with `arguments` on the configuration of the agent in `herdr`'s pane,
    /// outside the pane.
    fn run(herdr: &IsolatedHerdrServer, program: &Path, arguments: &[&str]) {
        let owner_s = owner_s_claude_variables();
        let output = Command::new(program)
            .args(arguments)
            .current_dir(herdr.server.root())
            .env_clear()
            .envs(herdr.server.environment().iter().filter(|(name, _)| {
                !owner_s
                    .iter()
                    .any(|owner_s| name.as_os_str() == owner_s.as_str())
            }))
            .envs(real_variables(herdr.server.root()))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{} {} failed:\n{}{}",
            program.display(),
            arguments.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// Changes the JSON file `name` of the configuration as `change` says.
    fn edit(&self, name: &str, change: impl FnOnce(&mut serde_json::Value)) {
        let path = self.0.join(name);
        let mut value = fs::read(&path).map_or_else(
            |_| serde_json::json!({}),
            |content| serde_json::from_slice(&content).unwrap(),
        );
        change(&mut value);
        fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    }
}
