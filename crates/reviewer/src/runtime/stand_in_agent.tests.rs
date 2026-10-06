//! What the stand-in agent shows Herdr and reports on the test's event socket
//! (`review_test_support::stand_in`): its turns, its title and its screen.
//!
//! A test that names an event socket controls the agent's turns: a turn ends once Herdr saw it
//! start, or, while the test holds turns, when the test ends it. Herdr sees a turn through the
//! agent's title, so a turn of an agent whose state is reported to Herdr instead, until the
//! test releases it, ends at once. Without an event socket, a turn lasts 1.5 seconds: the
//! tests that name none still count on it.

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use herdr_client::client::{EventCanceller, HerdrClient};
use herdr_client::protocol::{AgentStatus, HerdrEvent, PaneId};
use review_test_support::stand_in::{StandInCommand, StandInConnection, StandInEvent};

/// How long a turn lasts for a test that names no event socket.
const LEGACY_TURN: Duration = Duration::from_millis(1500);

/// The test's event socket, if it named one.
pub(super) struct Report(Option<StandInConnection>);

impl Report {
    pub(super) fn new(connection: Option<StandInConnection>) -> Self {
        Self(connection)
    }

    pub(super) fn send(&self, event: &StandInEvent) {
        if let Some(connection) = &self.0 {
            connection.report(event);
        }
    }

    fn controls_turns(&self) -> bool {
        self.0.is_some()
    }
}

/// The agent's turn.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Turn {
    /// No turn is under way.
    #[default]
    None,
    /// A turn is under way, and the agent shows it; Herdr has not reported it working since.
    Unseen,
    /// A turn is under way, and Herdr reported the agent working since it started.
    Seen,
}

/// Where the agent's turns stand.
#[derive(Default)]
struct TurnState {
    turn: Turn,
    /// Turns end only when the test ends them.
    held: bool,
    /// The status Herdr reported last.
    status: Option<AgentStatus>,
    /// Herdr reads the agent's title: its state is not reported, or the test released it.
    title_read: bool,
    /// For a test that names no event socket: until when the agent works.
    legacy_until: Option<Instant>,
}

/// The turns the agent starts on the prompts it reads.
#[derive(Clone)]
pub(super) struct AgentTurns {
    /// While this file exists, the agent reads each prompt without starting on it.
    swallow_path: PathBuf,
    state: Arc<Mutex<TurnState>>,
    display: Sender<()>,
    report: Arc<Report>,
}

impl AgentTurns {
    /// The turns of an agent whose state Herdr reads from its title when `title_read`, and the
    /// receiver the display wakes on.
    pub(super) fn new(
        swallow_path: PathBuf,
        title_read: bool,
        report: Arc<Report>,
    ) -> (Self, Receiver<()>) {
        let (display, wakes) = mpsc::channel();
        let state = TurnState {
            title_read,
            ..TurnState::default()
        };
        let turns = Self {
            swallow_path,
            state: Arc::new(Mutex::new(state)),
            display,
            report,
        };
        (turns, wakes)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, TurnState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A real agent starts a turn on a prompt, which Herdr sees; this one does too, unless the
    /// test makes it swallow its prompts.
    pub(super) fn prompt_read(&self) {
        if self.swallow_path.exists() {
            return;
        }
        let mut state = self.lock();
        if !self.report.controls_turns() {
            state.legacy_until = Some(Instant::now() + LEGACY_TURN);
            drop(state);
            let _ = self.display.send(());
            return;
        }
        if state.turn == Turn::None {
            // Herdr that reports the agent working already answers the prompt at once.
            state.turn = if state.status == Some(AgentStatus::Working) {
                Turn::Seen
            } else {
                Turn::Unseen
            };
            self.report.send(&StandInEvent::TurnStarted);
        }
        self.settle(&mut state);
    }

    /// Ends the turn under way if it may end now.
    fn settle(&self, state: &mut TurnState) {
        let ends = match state.turn {
            Turn::None => false,
            Turn::Unseen => !state.title_read,
            Turn::Seen => true,
        };
        if ends && !state.held {
            self.finish(state);
        } else {
            let _ = self.display.send(());
        }
    }

    fn finish(&self, state: &mut TurnState) {
        state.turn = Turn::None;
        self.report.send(&StandInEvent::TurnFinished);
        let _ = self.display.send(());
    }

    /// Whether the agent shows a turn under way.
    pub(super) fn working(&self) -> bool {
        let state = self.lock();
        state.turn != Turn::None
            || state
                .legacy_until
                .is_some_and(|until| Instant::now() < until)
    }

    /// Herdr reported the agent `status`.
    fn herdr_reported(&self, status: AgentStatus) {
        let mut state = self.lock();
        state.status = Some(status);
        if status == AgentStatus::Working && state.turn == Turn::Unseen {
            state.turn = Turn::Seen;
        }
        self.settle(&mut state);
    }

    /// Herdr reads the agent's title from now on.
    fn released(&self) {
        let mut state = self.lock();
        state.title_read = true;
        drop(state);
        let _ = self.display.send(());
    }

    /// Follows what the test says, and what Herdr reports of the agent of `pane`, for as long
    /// as the agent runs.
    pub(super) fn follow(
        &self,
        commands: Option<Receiver<StandInCommand>>,
        display: &DisplayFiles,
        pane: &PaneId,
    ) {
        let Some(commands) = commands else {
            return;
        };
        let client = HerdrClient::new(
            std::env::var_os("HERDR_SOCKET_PATH").unwrap().into(),
            "stand-in".into(),
            std::env::temp_dir(),
        );
        let canceller = EventCanceller::default();
        let statuses = client.subscribe_agent_status(pane, &canceller).unwrap();
        let events = client.subscribe_events(&canceller).unwrap();
        let turns = self.clone();
        thread::spawn(move || {
            statuses.forward(|status| {
                turns.herdr_reported(status);
                true
            })
        });
        let (turns, own) = (self.clone(), pane.clone());
        thread::spawn(move || {
            events.forward(|event| {
                if let HerdrEvent::AgentDetected {
                    pane_id, released, ..
                } = event
                    && pane_id == own
                {
                    if released {
                        turns.released();
                    }
                    // Herdr misses a title shown before it detected the agent.
                    let _ = turns.display.send(());
                }
                true
            })
        });
        let (turns, display) = (self.clone(), display.clone());
        thread::spawn(move || {
            for command in commands {
                turns.obey(command, &display);
            }
        });
    }

    fn obey(&self, command: StandInCommand, display: &DisplayFiles) {
        match command {
            StandInCommand::HoldTurns { hold } => {
                let mut state = self.lock();
                state.held = hold;
                self.settle(&mut state);
            }
            StandInCommand::EndTurn => {
                let mut state = self.lock();
                if state.turn != Turn::None {
                    self.finish(&mut state);
                }
            }
            StandInCommand::ShowTitle { title } => {
                fs::write(&display.title, title).unwrap();
                let _ = self.display.send(());
            }
            StandInCommand::ShowScreen { text } => {
                fs::write(&display.screen, text).unwrap();
                let _ = self.display.send(());
            }
            StandInCommand::Submit | StandInCommand::End => {
                panic!("the stand-in agent got a fork's command: {command:?}")
            }
        }
    }
}

/// The files that hold what the agent shows: the title the test gave it, and its screen.
#[derive(Clone)]
pub(super) struct DisplayFiles {
    pub(super) title: PathBuf,
    pub(super) screen: PathBuf,
}

/// What the test agent shows Herdr: the title the test gave it, or a working title while the
/// agent works on a prompt, and the screen the test gave it.
pub(super) struct AgentDisplay {
    pub(super) files: DisplayFiles,
    pub(super) turns: AgentTurns,
    /// Wakes the display when what it shows changed.
    pub(super) wakes: Receiver<()>,
}

impl AgentDisplay {
    pub(super) fn run(self) {
        let mut previous = None;
        let mut previous_screen = String::new();
        let mut shown_at = Instant::now();
        let mut again = false;
        loop {
            let title = self.title(previous.as_ref());
            // Shown again now and then: Herdr misses a title shown before it detects the agent.
            if title.is_some()
                && (title != previous
                    || std::mem::take(&mut again)
                    || shown_at.elapsed() > Duration::from_secs(1))
            {
                shown_at = Instant::now();
                print!("\x1b]0;{}\x07", title.as_deref().unwrap_or_default());
                io::stdout().flush().unwrap();
                previous = title;
            }
            if let Ok(screen) = fs::read_to_string(&self.files.screen)
                && screen != previous_screen
            {
                print!("\x1b[2J\x1b[H{}", screen.replace('\n', "\r\n"));
                io::stdout().flush().unwrap();
                previous_screen = screen;
            }
            // A test that names no event socket writes the files, which this reads again.
            if let Ok(()) = self.wakes.recv_timeout(Duration::from_millis(25)) {
                // Shown again: the wake may be Herdr detecting the agent.
                again = true;
            }
        }
    }

    /// The title to show after `previous`: none before the test gives one, and an empty one
    /// once a working title ends with no title given.
    fn title(&self, previous: Option<&String>) -> Option<String> {
        if self.turns.working() {
            return Some("⠋ Working".to_owned());
        }
        fs::read_to_string(&self.files.title)
            .ok()
            .or_else(|| previous.map(|_| String::new()))
    }
}
