//! The stand-in agent in its pane: it reads the prompts typed there, and starts a turn on each.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use herdr_client::protocol::PaneId;
use review_test_support::stand_in::{StandInConnection, StandInEvent, StandInRole, marker_of};
use stand_in_agent::{AGENT_SOURCE, StandInDirectory, variables};

use crate::hooks::Hooks;
use crate::input::AgentInput;
use crate::transcript::StandInTranscript;
use crate::turns::{AgentDisplay, AgentTurns, draw};

/// Runs the agent until Ctrl-D.
pub(crate) fn run() {
    let directory = StandInDirectory::new(PathBuf::from(
        std::env::var_os(variables::DIRECTORY).expect("the stand-in's directory"),
    ));
    // Match a native TUI: the terminal must not submit pasted newlines, echo
    // input, or truncate long lines through its canonical input buffer.
    crossterm::terminal::enable_raw_mode().unwrap();
    draw("\x1b[?2004h");
    let herdr = std::env::var_os(variables::HERDR)
        .or_else(|| std::env::var_os("HERDR_BIN_PATH"))
        .map_or_else(|| PathBuf::from("herdr"), PathBuf::from);
    let pane_id = std::env::var("HERDR_PANE_ID").unwrap();
    let agent = std::env::var(variables::AGENT).unwrap();
    let role = if std::env::var(variables::ROLE).as_deref() == Ok("second") {
        StandInRole::SecondAgent
    } else {
        StandInRole::Agent
    };
    let (connection, commands) = StandInConnection::from_env(role).expect("the event socket");
    let report = Arc::new(connection);
    let reported_lifecycle = std::env::var(variables::REPORT_LIFECYCLE).as_deref() != Ok("0");
    let (turns, wakes) = AgentTurns::new(
        directory.swallow(),
        !reported_lifecycle,
        Arc::clone(&report),
    );
    // Follows Herdr before it reports anything, so that it sees the test release it.
    turns.follow(commands, &PaneId(pane_id.clone()));
    if reported_lifecycle {
        let status = Command::new(&herdr)
            .args([
                "pane",
                "report-agent",
                &pane_id,
                "--source",
                AGENT_SOURCE,
                "--agent",
                &agent,
                "--state",
                "idle",
            ])
            .status()
            .unwrap();
        assert!(status.success());
        report.report(&StandInEvent::StateReported {
            state: "idle".into(),
        });
    }
    let mut hooks = Hooks {
        herdr,
        pane: pane_id,
        agent: agent.clone(),
        control: std::env::var_os(variables::HOOKS).map(PathBuf::from),
        session: String::new(),
        report: Arc::clone(&report),
    };
    if let Ok(session) = std::env::var(variables::SESSION)
        && !session.is_empty()
    {
        hooks.session_started(&session, "startup");
    }
    let display = AgentDisplay {
        turns: turns.clone(),
        wakes,
    };
    thread::spawn(move || display.run());
    let mut prompts = AgentPrompts {
        directory,
        transcript: StandInTranscript::open(),
        turns: turns.clone(),
        hooks,
        report: Arc::clone(&report),
    };
    let marker = if agent == "claude" { "❯" } else { "›" };
    draw(&format!("\x1b[2J\x1b[H{marker} "));
    let mut input = AgentInput::default();
    loop {
        let event = crossterm::event::read().unwrap();
        if matches!(event, crossterm::event::Event::Key(key)
            if key.code == crossterm::event::KeyCode::Char('d')
                && key.modifiers == crossterm::event::KeyModifiers::CONTROL)
        {
            crossterm::terminal::disable_raw_mode().unwrap();
            draw("\x1b[?2004l");
            report.report(&StandInEvent::Exited);
            return;
        }
        if let Some(prompt) = input.handle(event) {
            input.restore(prompts.read(&prompt));
        }
        let screen = if input.text().is_empty() {
            turns.screen().unwrap_or_else(|| format!("{marker} "))
        } else {
            format!("{marker} {}", input.text())
        };
        draw(&format!("\x1b[2J\x1b[H{}", screen.replace('\n', "\r\n")));
    }
}

/// The prompts the agent reads, and what it does with each.
struct AgentPrompts {
    directory: StandInDirectory,
    transcript: StandInTranscript,
    turns: AgentTurns,
    hooks: Hooks,
    report: Arc<StandInConnection>,
}

impl AgentPrompts {
    /// Claude Code's own `/resume <session>` continues that session; a marker only says that
    /// the agent read it; any other prompt starts a turn, unless the hooks block it: Claude
    /// Code 2.1.309 then puts it back in its input box, and this returns it.
    fn read(&mut self, prompt: &str) -> Option<String> {
        if let Some(id) = marker_of(prompt) {
            self.report.report(&StandInEvent::Marker { id });
            return None;
        }
        if let Some(session) = prompt.trim().strip_prefix("/resume ") {
            self.transcript.resume(session);
            self.report.report(&StandInEvent::ResumeReceived {
                session: session.to_owned(),
            });
            let unreported = self.directory.unreported_resume();
            match fs::read_to_string(&unreported) {
                Err(_) => self.hooks.session_started(session, "resume"),
                Ok(late) => {
                    fs::remove_file(&unreported).unwrap();
                    if let Ok(late) = late.trim().parse() {
                        thread::sleep(Duration::from_millis(late));
                        self.hooks.session_started(session, "resume");
                    }
                }
            }
            return None;
        }
        if self.hooks.blocks(prompt) {
            self.report.report(&StandInEvent::PromptBlocked {
                text: prompt.to_owned(),
            });
            return Some(prompt.to_owned());
        }
        self.transcript.turn(prompt);
        let swallowed = self.turns.swallows();
        // The agent reads the turn's files as it reads the prompt that names them.
        self.report.report(&StandInEvent::PromptReceived {
            text: review_test_support::turn::with_turn(prompt),
        });
        self.turns.prompt_read(swallowed);
        None
    }
}
