//! Terminal and worker integration for the review pane.

use std::env;
use std::io::{self, stdout};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
    MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use pr_core::diff::parse_file_diff;
use pr_core::herdr::{AgentTarget, PaneId, PluginContext, WorkspaceId};
use pr_core::herdr_client::HerdrClient;
use pr_core::repository::{ChangedFile, PollResult, Repository, Snapshot};
use pr_state::ReviewStore;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use signal_hook::consts::signal::{SIGHUP, SIGINT, SIGTERM};
use signal_hook::flag;

use crate::review::{MarkResult, ReviewTracker, ReviewWarning};
use crate::theme::Theme;
use crate::ui::{Action, Key, Message, ReviewApp, ReviewFile};

const POLL_INTERVAL: Duration = Duration::from_secs(2);
const EVENT_WAIT: Duration = Duration::from_millis(50);

/// The running review pane.
#[derive(Debug)]
pub struct Runtime {
    repository: Repository,
    state_dir: PathBuf,
    workspace_id: WorkspaceId,
    initial_agent: Option<PaneId>,
    client: HerdrClient,
    theme: Theme,
}

#[derive(Debug)]
struct Worker {
    repository: Repository,
    tracker: ReviewTracker,
    client: HerdrClient,
    target: AgentTarget,
    snapshot: Option<Snapshot>,
}

#[derive(Debug)]
enum WorkerCommand {
    Poll,
    LoadDiff { commit_id: String, path: String },
    SetReviewed { path: String, reviewed: bool },
    Insert(String),
    Focus(PaneId),
    Quit,
}

struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

impl Runtime {
    /// Read the pane context supplied by Herdr.
    pub fn from_env() -> eyre::Result<Self> {
        let repository = Repository::discover(env::current_dir()?)?;
        let context: PluginContext = serde_json::from_str(
            &env::var("HERDR_PLUGIN_CONTEXT_JSON")
                .map_err(|_| eyre::eyre!("HERDR_PLUGIN_CONTEXT_JSON is not set"))?,
        )?;
        Ok(Self {
            repository,
            state_dir: env::var_os("HERDR_PLUGIN_STATE_DIR")
                .map(PathBuf::from)
                .ok_or_else(|| eyre::eyre!("HERDR_PLUGIN_STATE_DIR is not set"))?,
            workspace_id: WorkspaceId(
                env::var("HERDR_WORKSPACE_ID")
                    .map_err(|_| eyre::eyre!("HERDR_WORKSPACE_ID is not set"))?,
            ),
            initial_agent: context.focused_pane_id,
            client: HerdrClient::from_env()?,
            theme: Theme::from_env()?,
        })
    }

    /// Run until the user quits or Herdr stops the pane.
    pub fn run(self) -> eyre::Result<()> {
        let stopped = Arc::new(AtomicBool::new(false));
        for signal in [SIGINT, SIGTERM, SIGHUP] {
            flag::register(signal, Arc::clone(&stopped))?;
        }

        let (commands, messages, worker) = self.start_worker()?;
        let (focus_sender, focus_events) = mpsc::channel();
        let event_client = self.client.clone();
        thread::spawn(move || {
            let _ = event_client.forward_focus_events(&focus_sender);
        });

        let mut terminal = TerminalGuard::new()?;
        let mut app = ReviewApp::with_theme(self.theme);
        commands.send(WorkerCommand::Poll)?;
        let mut next_poll = Instant::now() + POLL_INTERVAL;
        let result = loop {
            Self::drain_focus(&commands, &focus_events);
            if Self::drain_messages(&commands, &messages, &mut app)? {
                break Ok(());
            }
            if stopped.load(Ordering::Relaxed) {
                break Ok(());
            }
            if Instant::now() >= next_poll {
                commands.send(WorkerCommand::Poll)?;
                next_poll = Instant::now() + POLL_INTERVAL;
            }
            let area = terminal.terminal.size()?;
            let _ = app.update(Message::Resize {
                width: area.width,
                height: area.height,
            });
            terminal
                .terminal
                .draw(|frame| frame.render_widget(app.view(), frame.area()))?;
            if event::poll(EVENT_WAIT)? {
                let message = match event::read()? {
                    Event::Key(key) => normalize_key(key).map(Message::Key),
                    Event::Mouse(mouse) => normalize_mouse(mouse),
                    _ => None,
                };
                if let Some(message) = message
                    && Self::dispatch(&commands, app.update(message))?
                {
                    break Ok(());
                }
            }
        };
        self.repository.cancel();
        drop(terminal);
        let _ = commands.send(WorkerCommand::Quit);
        let _ = worker.join();
        result
    }

    fn start_worker(
        &self,
    ) -> eyre::Result<(Sender<WorkerCommand>, Receiver<Message>, JoinHandle<()>)> {
        let store = ReviewStore::open(&self.state_dir, self.repository.root())?;
        let tracker = ReviewTracker::new(self.repository.clone(), store);
        let mut target = AgentTarget::new(self.workspace_id.clone());
        if let Some(pane_id) = &self.initial_agent {
            target.observe_focus(&self.client, pane_id)?;
        } else {
            target.initialize(&self.client)?;
        }
        let mut worker = Worker {
            repository: self.repository.clone(),
            tracker,
            client: self.client.clone(),
            target,
            snapshot: None,
        };
        let (command_sender, command_receiver) = mpsc::channel();
        let (message_sender, message_receiver) = mpsc::channel();
        let handle = thread::spawn(move || worker.run(&command_receiver, &message_sender));
        Ok((command_sender, message_receiver, handle))
    }

    fn drain_focus(commands: &Sender<WorkerCommand>, events: &Receiver<PaneId>) {
        while let Ok(pane_id) = events.try_recv() {
            let _ = commands.send(WorkerCommand::Focus(pane_id));
        }
    }

    fn drain_messages(
        commands: &Sender<WorkerCommand>,
        messages: &Receiver<Message>,
        app: &mut ReviewApp,
    ) -> eyre::Result<bool> {
        loop {
            match messages.try_recv() {
                Ok(message) => {
                    if Self::dispatch(commands, app.update(message))? {
                        return Ok(true);
                    }
                }
                Err(TryRecvError::Empty) => return Ok(false),
                Err(TryRecvError::Disconnected) => {
                    eyre::bail!("review worker stopped unexpectedly");
                }
            }
        }
    }

    fn dispatch(commands: &Sender<WorkerCommand>, action: Action) -> eyre::Result<bool> {
        let command = match action {
            Action::None => return Ok(false),
            Action::Quit => return Ok(true),
            Action::LoadDiff { commit_id, path } => WorkerCommand::LoadDiff { commit_id, path },
            Action::SetReviewed { path, reviewed } => WorkerCommand::SetReviewed { path, reviewed },
            Action::Insert { text } => WorkerCommand::Insert(text),
        };
        commands.send(command)?;
        Ok(false)
    }
}

impl Worker {
    fn run(&mut self, commands: &Receiver<WorkerCommand>, messages: &Sender<Message>) {
        while let Ok(command) = commands.recv() {
            let keep_running = match command {
                WorkerCommand::Poll => {
                    self.poll(messages);
                    true
                }
                WorkerCommand::LoadDiff { commit_id, path } => {
                    self.load_diff(messages, commit_id, path);
                    true
                }
                WorkerCommand::SetReviewed { path, reviewed } => {
                    self.set_reviewed(messages, path, reviewed);
                    true
                }
                WorkerCommand::Insert(text) => {
                    let result = self
                        .target
                        .insert(&self.client, &text)
                        .map_err(|error| error.to_string());
                    let _ = messages.send(Message::InsertFinished(result));
                    true
                }
                WorkerCommand::Focus(pane_id) => {
                    let _ = self.target.observe_focus(&self.client, &pane_id);
                    true
                }
                WorkerCommand::Quit => false,
            };
            if !keep_running {
                return;
            }
        }
    }

    fn poll(&mut self, messages: &Sender<Message>) {
        let snapshot = match self.repository.poll() {
            Ok(PollResult::Complete(snapshot)) => snapshot,
            Ok(PollResult::ChangedDuringPoll) => return,
            Err(error) => {
                let _ = messages.send(Message::PollFailed(error.to_string()));
                return;
            }
        };
        let states = snapshot
            .files
            .iter()
            .map(|file| self.tracker.status(&snapshot, file))
            .collect::<eyre::Result<Vec<_>>>();
        let warning = states
            .as_ref()
            .ok()
            .and_then(|states| states.iter().find_map(|state| state.warning));
        let files = states.map(|states| {
            snapshot
                .files
                .iter()
                .zip(states)
                .map(|(file, state)| ReviewFile::from_changed(file, state.status))
                .collect::<Vec<_>>()
        });
        match files {
            Ok(files) => {
                let _ = messages.send(Message::FilesLoaded {
                    change_id: snapshot.identity.change_id.as_str().to_owned(),
                    commit_id: snapshot.identity.commit_id.as_str().to_owned(),
                    files,
                });
                if let Some(warning) = warning {
                    let text = match warning {
                        ReviewWarning::UnknownSchema => {
                            "Review state uses an unknown schema; file is unreviewed"
                        }
                        ReviewWarning::BaselineExpired => {
                            "Review baseline expired; file reset to unreviewed"
                        }
                    };
                    let _ = messages.send(Message::PollFailed(text.to_owned()));
                }
                self.snapshot = Some(snapshot);
            }
            Err(error) => {
                let _ = messages.send(Message::PollFailed(error.to_string()));
            }
        }
    }

    fn load_diff(&self, messages: &Sender<Message>, commit_id: String, path: String) {
        let result = self
            .find_file(&commit_id, &path)
            .and_then(|(snapshot, file)| {
                let diff = self.tracker.diff(snapshot, file)?;
                Ok((
                    parse_file_diff(&diff.unified, file),
                    diff.old_content,
                    diff.new_content,
                ))
            });
        let message = match result {
            Ok((rows, old_content, new_content)) => Message::DiffLoaded {
                commit_id,
                path,
                rows,
                old_content,
                new_content,
            },
            Err(error) => Message::DiffFailed {
                commit_id,
                path,
                message: error.to_string(),
            },
        };
        let _ = messages.send(message);
    }

    fn set_reviewed(&self, messages: &Sender<Message>, path: String, reviewed: bool) {
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        let change_id = snapshot.identity.change_id.as_str().to_owned();
        let result = snapshot
            .files
            .iter()
            .find(|file| file.display_path == path)
            .ok_or_else(|| eyre::eyre!("the selected file is no longer in the current change"))
            .and_then(|file| {
                if reviewed {
                    match self.tracker.mark(snapshot, file)? {
                        MarkResult::Marked => self.tracker.status(snapshot, file),
                        MarkResult::ChangeChanged => {
                            eyre::bail!("the change moved; wait for the next refresh")
                        }
                    }
                } else {
                    self.tracker.unreview(snapshot, file)?;
                    self.tracker.status(snapshot, file)
                }
            })
            .map_err(|error| error.to_string());
        let _ = messages.send(Message::ReviewFinished {
            change_id,
            path,
            result,
        });
    }

    fn find_file<'a>(
        &'a self,
        commit_id: &str,
        path: &str,
    ) -> eyre::Result<(&'a Snapshot, &'a ChangedFile)> {
        let snapshot = self
            .snapshot
            .as_ref()
            .filter(|snapshot| snapshot.identity.commit_id.as_str() == commit_id)
            .ok_or_else(|| eyre::eyre!("the diff snapshot is no longer current"))?;
        let file = snapshot
            .files
            .iter()
            .find(|file| file.display_path == path)
            .ok_or_else(|| eyre::eyre!("the selected file is no longer in the current change"))?;
        Ok((snapshot, file))
    }
}

impl TerminalGuard {
    fn new() -> eyre::Result<Self> {
        enable_raw_mode()?;
        let mut terminal = match Terminal::new(CrosstermBackend::new(stdout())) {
            Ok(terminal) => terminal,
            Err(error) => {
                let _ = disable_raw_mode();
                return Err(error.into());
            }
        };
        if let Err(error) = execute!(
            terminal.backend_mut(),
            EnterAlternateScreen,
            EnableMouseCapture
        ) {
            let _ = disable_raw_mode();
            let _ = execute!(
                terminal.backend_mut(),
                DisableMouseCapture,
                LeaveAlternateScreen
            );
            return Err(error.into());
        }
        Ok(Self { terminal })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            DisableMouseCapture,
            LeaveAlternateScreen
        );
        let _ = self.terminal.show_cursor();
    }
}

fn normalize_mouse(mouse: MouseEvent) -> Option<Message> {
    let (column, row) = (mouse.column, mouse.row);
    let step = if mouse.modifiers.contains(KeyModifiers::SHIFT) {
        6
    } else {
        3
    };
    match mouse.kind {
        MouseEventKind::ScrollUp => Some(Message::MouseScroll {
            column,
            row,
            delta: -step,
        }),
        MouseEventKind::ScrollDown => Some(Message::MouseScroll {
            column,
            row,
            delta: step,
        }),
        MouseEventKind::Down(MouseButton::Left) => Some(Message::MouseClick {
            column,
            row,
            insert_path: mouse.modifiers.contains(KeyModifiers::SHIFT)
                || mouse.modifiers.contains(KeyModifiers::CONTROL),
        }),
        MouseEventKind::Down(MouseButton::Middle) => Some(Message::MouseClick {
            column,
            row,
            insert_path: true,
        }),
        _ => None,
    }
}

fn normalize_key(key: KeyEvent) -> Option<Key> {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            KeyCode::Char('d') => Some(Key::HalfPageDown),
            KeyCode::Char('u') => Some(Key::HalfPageUp),
            _ => None,
        };
    }
    match key.code {
        KeyCode::Tab => Some(Key::Tab),
        KeyCode::Down | KeyCode::Char('j') => Some(Key::Down),
        KeyCode::Up | KeyCode::Char('k') => Some(Key::Up),
        KeyCode::Home | KeyCode::Char('g') => Some(Key::First),
        KeyCode::End | KeyCode::Char('G') => Some(Key::Last),
        KeyCode::Char('v' | 'V') => Some(Key::Visual),
        KeyCode::Esc => Some(Key::Escape),
        KeyCode::Enter => Some(Key::Enter),
        KeyCode::Char(' ') => Some(Key::Space),
        KeyCode::Char('q') => Some(Key::Quit),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modified_mouse_inputs_reuse_existing_actions() {
        assert_eq!(
            normalize_key(KeyEvent::new(KeyCode::Char('V'), KeyModifiers::SHIFT)),
            Some(Key::Visual)
        );
        assert_eq!(
            normalize_mouse(MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: 4,
                row: 5,
                modifiers: KeyModifiers::SHIFT,
            }),
            Some(Message::MouseScroll {
                column: 4,
                row: 5,
                delta: 6,
            })
        );
        assert_eq!(
            normalize_mouse(MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: 4,
                row: 5,
                modifiers: KeyModifiers::NONE,
            }),
            Some(Message::MouseScroll {
                column: 4,
                row: 5,
                delta: 3,
            })
        );
        for (kind, modifiers) in [
            (MouseEventKind::Down(MouseButton::Left), KeyModifiers::SHIFT),
            (
                MouseEventKind::Down(MouseButton::Left),
                KeyModifiers::CONTROL,
            ),
            (
                MouseEventKind::Down(MouseButton::Middle),
                KeyModifiers::NONE,
            ),
        ] {
            assert_eq!(
                normalize_mouse(MouseEvent {
                    kind,
                    column: 4,
                    row: 5,
                    modifiers,
                }),
                Some(Message::MouseClick {
                    column: 4,
                    row: 5,
                    insert_path: true,
                })
            );
        }
    }
}
