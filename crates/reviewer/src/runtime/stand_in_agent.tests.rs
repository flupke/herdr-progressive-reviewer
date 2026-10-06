//! What the stand-in agent shows Herdr and reports on the test's event socket
//! (`review_test_support::stand_in`): its turns, its title and its screen.
//!
//! The test controls the agent's turns: a turn ends once Herdr saw it start, or, while the
//! test holds turns, when the test ends it. Herdr sees a turn through the agent's title, so a
//! turn of an agent whose state is reported to Herdr instead, until the test releases it, ends
//! at once.

use std::fs::File;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

use herdr_client::client::{EventCanceller, HerdrClient};
use herdr_client::protocol::{AgentStatus, HerdrEvent, PaneId};
use review_test_support::stand_in::{StandInCommand, StandInConnection, StandInEvent};

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

/// Where the agent's turns stand, and what the test gave it to show.
#[derive(Default)]
struct TurnState {
    turn: Turn,
    /// Turns end only when the test ends them.
    held: bool,
    /// The status Herdr reported last.
    status: Option<AgentStatus>,
    /// Herdr reads the agent's title: its state is not reported, or the test released it.
    title_read: bool,
    /// The title the test gave the agent, shown outside its turns.
    title: Option<String>,
    /// The screen the test gave the agent, shown while nothing is typed in it.
    screen: Option<String>,
}

/// The turns the agent starts on the prompts it reads.
#[derive(Clone)]
pub(super) struct AgentTurns {
    /// While this file exists, the agent reads each prompt without starting on it. The test
    /// writes it before the prompt it concerns, so the agent reads it in order.
    swallow_path: PathBuf,
    state: Arc<Mutex<TurnState>>,
    display: Sender<()>,
    report: Arc<StandInConnection>,
}

impl AgentTurns {
    /// The turns of an agent whose state Herdr reads from its title when `title_read`, and the
    /// receiver the display wakes on.
    pub(super) fn new(
        swallow_path: PathBuf,
        title_read: bool,
        report: Arc<StandInConnection>,
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
        if state.turn == Turn::None {
            // Herdr that reports the agent working already answers the prompt at once.
            state.turn = if state.status == Some(AgentStatus::Working) {
                Turn::Seen
            } else {
                Turn::Unseen
            };
            self.report.report(&StandInEvent::TurnStarted);
        }
        self.settle(&mut state);
    }

    /// Ends the turn under way if it may end now.
    fn settle(&self, state: &mut TurnState) {
        // Herdr does not see the turns of an agent whose state is reported.
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
        self.report.report(&StandInEvent::TurnFinished);
        let _ = self.display.send(());
    }

    /// The title to show after `previous`: a working title during a turn, none before the
    /// test gives one, and an empty one once a working title ends with no title given.
    fn title(&self, previous: Option<&String>) -> Option<String> {
        let state = self.lock();
        if state.turn != Turn::None {
            return Some("⠋ Working".to_owned());
        }
        state
            .title
            .clone()
            .or_else(|| previous.map(|_| String::new()))
    }

    /// The screen the test gave the agent, if any.
    pub(super) fn screen(&self) -> Option<String> {
        self.lock().screen.clone()
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
        self.report.report(&StandInEvent::Released);
        drop(state);
        let _ = self.display.send(());
    }

    /// Follows what the test says, and what Herdr reports of the agent of `pane`, for as long
    /// as the agent runs.
    pub(super) fn follow(&self, commands: Receiver<StandInCommand>, pane: &PaneId) {
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
                    // Herdr misses a title shown before it detected the agent: shown again.
                    let _ = turns.display.send(());
                }
                true
            })
        });
        let turns = self.clone();
        thread::spawn(move || {
            for command in commands {
                turns.obey(command);
            }
        });
    }

    fn obey(&self, command: StandInCommand) {
        let mut state = self.lock();
        match command {
            StandInCommand::HoldTurns { hold } => {
                state.held = hold;
                self.report.report(&StandInEvent::TurnsHeld { hold });
                self.settle(&mut state);
            }
            StandInCommand::EndTurn => {
                if state.turn != Turn::None {
                    self.finish(&mut state);
                }
            }
            StandInCommand::Release => {
                drop(state);
                self.released();
            }
            StandInCommand::ShowTitle { title } => {
                state.title = Some(title);
                let _ = self.display.send(());
            }
            StandInCommand::ShowScreen { text } => {
                state.screen = Some(text);
                let _ = self.display.send(());
            }
            StandInCommand::Submit | StandInCommand::End => {
                panic!("the stand-in agent got a fork's command: {command:?}")
            }
        }
    }
}

/// The agent's pane, which it draws in. The test harness that runs the agent writes on the
/// process's standard output, which goes nowhere: a warning that the agent ran for over a
/// minute is no text of the agent's.
#[derive(Clone)]
pub(super) struct Pane(Arc<Mutex<File>>);

impl Pane {
    /// Takes the pane from the standard output, which then goes to `/dev/null`.
    pub(super) fn take() -> Self {
        let pane = rustix::io::dup(io::stdout()).unwrap();
        let nowhere = File::options().write(true).open("/dev/null").unwrap();
        rustix::stdio::dup2_stdout(&nowhere).unwrap();
        Self(Arc::new(Mutex::new(File::from(pane))))
    }

    /// Writes `text` to the pane.
    pub(super) fn draw(&self, text: &str) {
        let mut pane = self.lock();
        pane.write_all(text.as_bytes()).unwrap();
        pane.flush().unwrap();
    }

    pub(super) fn lock(&self) -> std::sync::MutexGuard<'_, File> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// What the test agent shows Herdr: the screen the test gave it, then the title the test gave
/// it, or a working title while the agent works on a prompt. The screen goes first, so that
/// once Herdr reports the status a title shows, it shows the screen drawn before.
pub(super) struct AgentDisplay {
    pub(super) pane: Pane,
    pub(super) turns: AgentTurns,
    /// Wakes the display when what it shows changed, or when Herdr detected the agent.
    pub(super) wakes: Receiver<()>,
}

impl AgentDisplay {
    pub(super) fn run(self) {
        let mut previous_title = None;
        let mut previous_screen = None;
        // Shown again on each wake: the wake may be Herdr detecting the agent.
        while let Ok(()) = self.wakes.recv() {
            let screen = self.turns.screen();
            if screen.is_some() && screen != previous_screen {
                let text = screen.as_deref().unwrap_or_default();
                self.pane
                    .draw(&format!("\x1b[2J\x1b[H{}", text.replace('\n', "\r\n")));
                previous_screen = screen;
            }
            let title = self.turns.title(previous_title.as_ref());
            if let Some(shown) = &title {
                self.pane.draw(&format!("\x1b]0;{shown}\x07"));
                previous_title = title;
            }
        }
    }
}
