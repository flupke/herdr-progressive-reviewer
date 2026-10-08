//! Everything the runtime does outside the application: background workers, their
//! channels and their lifecycle.
//!
//! The event loop hands actions to [`Effects`] and receives every result as an event
//! on the [`Outputs`] channels. Explore autosave still comes from
//! the application as an action; this is where its effects would move.

use std::ops::ControlFlow;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};

use component_core::{ApplicationEventSender, EventEnvelope};
use crossbeam_channel::{Receiver as EventReceiver, Sender as EventSender};
use herdr_client::client::HerdrClient;
use herdr_client::protocol::{AgentTarget, HerdrEvent, PaneId};
use review_explore_page::RoundPublisher;
use review_explore_page_host::PageOpener;
use review_explore_session::{self as explore_session, ExploreSession};
use review_repository::repository::Repository;
use review_significance::JevClassifier;
use review_state::ReviewTracker;
use review_store::ReviewStore;
use review_thread_service as comments;
use review_ui::{
    Action, DocumentAction, DocumentLoad, ExplorePageAction, LspAction, RepositoryAction,
    SettingsAction, TerminalAction, Theme,
};

use super::actions::ActionExecutors;
use super::document;
use super::highlighting;
use super::page_sharing::PageSharing;
use super::page_threads::PageThreads;
use super::revision_progress::HistoryProgress;
use super::route::WorkerStopped;
use super::worker::{Worker, WorkerCommand};
use crate::watcher::SourceWatchRequests;

/// What the effects need from the process that runs them.
pub(super) struct Setup {
    pub(super) repository: Repository,
    pub(super) store: ReviewStore,
    pub(super) target: AgentTarget,
    pub(super) agents: HerdrClient,
    pub(super) endpoint: Result<review_mcp::Endpoint, String>,
    pub(super) theme: Theme,
    pub(super) jev: JevClassifier,
    /// Where a vision session reads the Explore prompts sent.
    pub(super) turns: Option<explore_session::TurnLog>,
    pub(super) source_watches: Option<SourceWatchRequests>,
    /// Where the Explore session publishes its round for the Explore page.
    pub(super) page: RoundPublisher,
    /// Opens the Explore page when the pane asks; `None` in tests, which never open a browser.
    pub(super) page_opener: Option<PageOpener>,
    /// The Explore page's link to the review threads, which hold the round's conversation.
    pub(super) page_threads: Arc<PageThreads>,
    /// How run-ahead forks the agent.
    pub(super) run_ahead: RunAheadSetup,
}

/// How run-ahead forks the agent's session.
pub(super) struct RunAheadSetup {
    /// The programs the forks start through.
    pub(super) tools: claude_fork::ForkTools,
    /// Where run-ahead writes its log; `None` writes none.
    pub(super) log: Option<PathBuf>,
    /// How long the forks' host waits on Herdr and on the forks.
    pub(super) waits: claude_fork::ForkWaits,
    /// How long the reviewer's talk with the agent in the round conversation stays quiet
    /// before run-ahead takes forks again.
    pub(super) talk_quiet: std::time::Duration,
    /// Where the hooks of the agents of Herdr's server reach the reviewer; `None` when it is
    /// not known.
    pub(super) hooks: Option<agent_hooks::HookDirectory>,
}

impl RunAheadSetup {
    /// The forks of the agents `agents` reports.
    fn forks(self, agents: &HerdrClient) -> Arc<claude_fork::ClaudeForks> {
        Arc::new(claude_fork::ClaudeForks::new(
            agents.clone(),
            self.tools,
            self.log,
            self.waits,
            self.hooks.as_ref(),
        ))
    }
}

/// Where effects deliver their results.
pub(super) struct Outputs {
    /// Results of slow or unrequested work.
    pub(super) background: EventSender<EventEnvelope>,
    /// Results the user waits for, which overtake background results.
    pub(super) interactive: EventSender<EventEnvelope>,
}

/// The running workers, reached only through actions and a few runtime signals.
///
/// Dropping it stops the workers and waits for them.
pub(super) struct Effects {
    repository_root: PathBuf,
    target: AgentTarget,
    store: ReviewStore,
    source_watches: Option<SourceWatchRequests>,
    commands: Sender<WorkerCommand>,
    documents: Sender<document::Command>,
    page_opener: Option<PageOpener>,
    page_threads: Arc<PageThreads>,
    /// Applies the network settings to the Explore page; `None` until the page is served, and
    /// in tests that serve no page.
    page_sharing: Option<PageSharing>,
    /// Where results of slow or unrequested work go.
    messages: ApplicationEventSender,
    /// Taken first on drop, so agent delivery stops before repository work does.
    front: Option<FrontWorkers>,
    lsp: review_lsp::Worker,
    threads: Vec<JoinHandle<()>>,
}

/// Workers that answer the UI directly, stopped before repository work.
struct FrontWorkers {
    comments: comments::Worker,
    search: text_search::Worker,
    highlighting: highlighting::Worker,
}

/// Runs terminal actions, which need the terminal the event loop owns.
type TerminalExecutor<'a> = dyn FnMut(TerminalAction) -> eyre::Result<ControlFlow<()>> + 'a;

impl Effects {
    pub(super) fn start(setup: Setup, outputs: &Outputs) -> Self {
        let Setup {
            repository,
            store,
            target,
            agents,
            endpoint,
            theme,
            jev,
            turns,
            source_watches,
            page,
            page_opener,
            page_threads,
            run_ahead,
        } = setup;
        let messages = ApplicationEventSender::new(outputs.background.clone());
        let tracker = Arc::new(ReviewTracker::new(repository.clone(), store.clone()));
        let (commands, command_receiver) = mpsc::channel();
        let (documents, document_receiver) = mpsc::channel();
        let mut document_worker = document::DocumentWorker {
            repository: repository.clone(),
            tracker: Arc::clone(&tracker),
            snapshot: None,
        };
        let document_messages = messages.clone();
        let document_thread = thread::spawn(move || {
            document_worker.run(&document_receiver, &document_messages);
            let _ = document_messages.send(WorkerStopped);
        });
        let comments = start_comments(
            &store,
            agents.clone(),
            target.clone(),
            endpoint,
            &commands,
            messages.clone(),
            Arc::clone(&page_threads),
        );
        let talk_quiet = run_ahead.talk_quiet;
        let forks = run_ahead.forks(&agents);
        let explore = ExploreSession::new(explore_session::Collaborators {
            repository: repository.clone(),
            store: store.clone(),
            tracker: Arc::clone(&tracker),
            agents: Arc::new(agents),
            target: target.clone(),
            prompts: comments.prompt_sender(),
            events: messages.clone(),
            inbox: inbox_for(commands.clone()),
            turns,
            page,
            forks,
            talk_quiet,
        });
        let mut worker = Worker {
            repository: repository.clone(),
            tracker,
            store: store.clone(),
            snapshot: None,
            commands: commands.clone(),
            explore,
            jev,
            auto_review: None,
            held_kickoff: None,
            documents: documents.clone(),
            revision_progress: HistoryProgress::default(),
        };
        let worker_messages = messages.clone();
        let worker_thread = thread::spawn(move || {
            worker.run(&command_receiver, &worker_messages);
            let _ = worker_messages.send(WorkerStopped);
        });
        let highlights = outputs.background.clone();
        let search_results = outputs.interactive.clone();
        Self {
            repository_root: repository.root().to_owned(),
            target,
            store,
            source_watches,
            commands,
            documents,
            page_opener,
            page_threads,
            page_sharing: None,
            messages,
            front: Some(FrontWorkers {
                comments,
                search: text_search::Worker::start(move |results| {
                    let _ = search_results.send(EventEnvelope::new(results));
                }),
                highlighting: highlighting::Worker::start(
                    syntax_highlighting::SyntaxHighlighter::new(theme.syntax, theme.palette.text),
                    move |result| {
                        let _ = highlights.send(EventEnvelope::new(result));
                    },
                ),
            }),
            lsp: review_lsp::Worker::start(repository.root().to_owned()),
            threads: vec![worker_thread, document_thread],
        }
    }

    /// Run actions in order until one stops the runtime.
    pub(super) fn perform_all(
        &self,
        actions: Vec<Action>,
        terminal: &mut TerminalExecutor<'_>,
    ) -> eyre::Result<ControlFlow<()>> {
        Performer {
            effects: self,
            terminal,
        }
        .run_all(actions)
    }

    /// Poll the repository; results arrive after earlier repository work.
    pub(super) fn refresh(&self) -> eyre::Result<()> {
        self.commands.send(WorkerCommand::Poll)?;
        Ok(())
    }

    /// The user focused a pane, which later prompts prefer as their agent.
    pub(super) fn agent_focused(&mut self, pane_id: &PaneId) {
        self.target.observe_focus(pane_id);
        self.front()
            .comments
            .send(comments::Command::ActiveAgentChanged);
    }

    /// Let agent delivery see Herdr traffic.
    pub(super) fn observe(&self, event: &HerdrEvent) {
        self.front()
            .comments
            .send(comments::Command::Observe(event.clone()));
    }

    fn front(&self) -> &FrontWorkers {
        self.front.as_ref().expect("front workers run until drop")
    }

    /// Apply later network settings to the Explore page through `sharing`.
    pub(super) fn share_page(&mut self, sharing: PageSharing) {
        self.page_sharing = Some(sharing);
    }

    /// Where inputs for the Explore session join repository work.
    pub(super) fn explore_inbox(&self) -> explore_session::Inbox {
        inbox_for(self.commands.clone())
    }

    /// Where the Explore page sends its thread commands, which write the round's conversation:
    /// to the thread worker, as the pane's do.
    pub(super) fn page_thread_sender(&self) -> review_explore_page::ThreadSender {
        self.page_threads
            .sender(self.front().comments.thread_commands())
    }

    /// Language-server events, which the runtime forwards on its own schedule.
    pub(super) fn lsp_events(&self) -> EventReceiver<review_lsp::Event> {
        self.lsp.event_receiver()
    }

    fn load_documents(&self, load: DocumentLoad) -> eyre::Result<()> {
        match load {
            // One command per path keeps each diff ordered against later snapshots.
            DocumentLoad::Diffs {
                review_checkpoint,
                paths,
            } => {
                for path in paths {
                    self.documents
                        .send(document::Command::Load(DocumentLoad::Diff {
                            review_checkpoint: review_checkpoint.clone(),
                            path,
                        }))?;
                }
            }
            DocumentLoad::Source {
                snapshot_id,
                mut location,
                mode,
            } => {
                location.path = self.resolve(location.path);
                self.documents
                    .send(document::Command::Load(DocumentLoad::Source {
                        snapshot_id,
                        location,
                        mode,
                    }))?;
            }
            load @ DocumentLoad::Diff { .. } => {
                self.documents.send(document::Command::Load(load))?;
            }
        }
        Ok(())
    }

    /// Open the Explore page in the browser, on a thread of its own since the browser program
    /// may take its time; the pane hears only of a failure.
    fn open_page(&self) {
        let Some(opener) = self.page_opener.clone() else {
            return;
        };
        let messages = self.messages.clone();
        thread::spawn(move || {
            if let Err(failure) = opener.open() {
                let _ = messages.send(ui_events::ExplorePageNotOpened(failure));
            }
        });
    }

    /// Shares the running round over a tunnel, or stops the tunnel. Without a page, there is
    /// none to share: the pane hears why, or that no tunnel runs.
    fn share_round(&self, on: bool) {
        use review_explore_page_host::TunnelState;
        match (&self.page_sharing, on) {
            (Some(sharing), true) => sharing.open_tunnel(),
            (Some(sharing), false) => sharing.close_tunnel(),
            (None, on) => {
                let state = if on {
                    TunnelState::Failed("the Explore page is not served".into())
                } else {
                    TunnelState::Off
                };
                let _ = self.messages.send(ui_events::ExplorePageTunnel(state));
            }
        }
    }

    /// Paths from the application are relative to the repository root.
    fn resolve(&self, path: PathBuf) -> PathBuf {
        if path.is_relative() {
            self.repository_root.join(path)
        } else {
            path
        }
    }

    /// Keep later repository work pending until the hold drops.
    #[cfg(test)]
    pub(super) fn hold_repository_work(&self) -> EventSender<()> {
        let (hold, release) = crossbeam_channel::bounded(0);
        self.commands
            .send(WorkerCommand::Hold(release))
            .expect("the repository worker is running");
        hold
    }

    /// Returns once the repository worker handled every command sent before, and every prompt
    /// the thread worker got by then is sent, withdrawn or failed.
    #[cfg(test)]
    pub(super) fn flush(&self) {
        self.hold_repository_work()
            .send(())
            .expect("the repository worker is running");
        self.front().comments.prompt_sender().flush();
    }

    #[cfg(test)]
    pub(super) fn replace_highlighting(&mut self, highlighting: highlighting::Worker) {
        self.front.as_mut().unwrap().highlighting = highlighting;
    }
}

impl Drop for Effects {
    fn drop(&mut self) {
        // No prompt or MCP request reaches the Explore session while it winds down.
        drop(self.front.take());
        let _ = self.commands.send(WorkerCommand::Quit);
        let _ = self.documents.send(document::Command::Quit);
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

/// Route inputs the Explore session produces later back to the worker's serial order.
fn inbox_for(commands: Sender<WorkerCommand>) -> explore_session::Inbox {
    explore_session::Inbox::new(move |input| {
        let _ = commands.send(WorkerCommand::Explore(input));
    })
}

/// Conversation traffic bypasses slow repository operations.
fn start_comments(
    store: &ReviewStore,
    agents: HerdrClient,
    target: AgentTarget,
    endpoint: Result<review_mcp::Endpoint, String>,
    commands: &Sender<WorkerCommand>,
    messages: ApplicationEventSender,
    page: Arc<PageThreads>,
) -> comments::Worker {
    let explore = inbox_for(commands.clone());
    let commands = commands.clone();
    comments::Worker::start(
        store.clone(),
        agents,
        target,
        endpoint,
        move |request| {
            commands
                .send(WorkerCommand::Explore(explore_session::Input::Submission(
                    Box::new(request),
                )))
                .map_err(|_| "The reviewer is closed".to_owned())
        },
        move |event| {
            page.observe(&event);
            if let comments::Event::RoundMessage(message) = &event {
                explore.deliver(explore_session::Input::RoundMessage(message.clone()));
            }
            publish_thread_event(&messages, event);
        },
    )
}

/// Shows the thread worker's `event` in the pane.
fn publish_thread_event(messages: &ApplicationEventSender, event: comments::Event) {
    match event {
        comments::Event::Loaded(event) => {
            let _ = messages.send(event);
        }
        comments::Event::Posted(event) => {
            let _ = messages.send(event);
        }
        comments::Event::Wakeup {
            failure: Some(failure),
            ..
        } => {
            let _ = messages.send(ui_events::ToastRequested {
                text: failure.error,
                kind: toasts::ToastKind::Error,
            });
        }
        comments::Event::Wakeup { failure: None, .. } | comments::Event::RoundMessage(_) => {}
        comments::Event::Error(text) => {
            let _ = messages.send(ui_events::ToastRequested {
                text,
                kind: toasts::ToastKind::Error,
            });
        }
    }
}

/// Hands each action group to the worker that runs it.
struct Performer<'a, 't> {
    effects: &'a Effects,
    terminal: &'a mut TerminalExecutor<'t>,
}

impl ActionExecutors for Performer<'_, '_> {
    fn explore(&mut self, command: review_explore::Command) -> eyre::Result<()> {
        self.effects
            .commands
            .send(WorkerCommand::Explore(explore_session::Input::Command(
                command,
            )))?;
        Ok(())
    }

    fn explore_page(&mut self, action: ExplorePageAction) -> eyre::Result<()> {
        match action {
            ExplorePageAction::Open => self.effects.open_page(),
            ExplorePageAction::OpenTunnel => self.effects.share_round(true),
            ExplorePageAction::CloseTunnel => self.effects.share_round(false),
        }
        Ok(())
    }

    fn thread(&mut self, command: review_threads::ThreadCommand) -> eyre::Result<()> {
        self.effects
            .front()
            .comments
            .send(comments::Command::Thread(command));
        Ok(())
    }

    fn document(&mut self, action: DocumentAction) -> eyre::Result<()> {
        let effects = self.effects;
        match action {
            DocumentAction::Load(load) => effects.load_documents(load)?,
            DocumentAction::Highlight(request) => effects
                .front()
                .highlighting
                .submit(request)
                .map_err(eyre::Report::msg)?,
            DocumentAction::Search(request) => effects.front().search.submit(request),
            DocumentAction::WatchSource(path) => {
                if let Some(watches) = &effects.source_watches {
                    watches.watch(path.as_deref());
                }
            }
        }
        Ok(())
    }

    fn lsp(&mut self, action: LspAction) -> eyre::Result<()> {
        let effects = self.effects;
        match action {
            LspAction::OpenDocument(path) => effects.lsp.open_document(path),
            LspAction::Request {
                operation,
                mut query,
            } => {
                query.path = effects.resolve(query.path);
                effects.lsp.request(operation, query)
            }
            LspAction::Restart => effects.lsp.restart(),
        }
        .map_err(eyre::Report::msg)
    }

    fn settings(&mut self, action: SettingsAction) -> eyre::Result<()> {
        let settings = &self.effects.store;
        match action {
            SettingsAction::SaveFilePaneWidth(columns) => settings.save_file_pane_width(columns)?,
            SettingsAction::SaveEditorKeymap(keymap) => settings.save_editor_keymap(keymap)?,
            SettingsAction::SaveExplorePage(setting) => {
                let saved = settings.save_explore_page_setting(setting)?;
                if let Some(sharing) = &self.effects.page_sharing {
                    sharing.apply(&saved.network);
                }
                let _ = self
                    .effects
                    .messages
                    .send(ui_events::ExplorePageSettingsLoaded(saved));
            }
            SettingsAction::SaveExploreWritingStyle(writing) => {
                let saved = settings.save_explore_writing_style(writing)?;
                let _ = self
                    .effects
                    .messages
                    .send(ui_events::ExploreRoundSettingsLoaded(saved));
            }
            SettingsAction::SaveExploreRunAhead(run_ahead) => {
                let saved = settings.save_explore_run_ahead(run_ahead)?;
                let _ = self
                    .effects
                    .messages
                    .send(ui_events::ExploreRoundSettingsLoaded(saved));
            }
        }
        Ok(())
    }

    fn repository(&mut self, action: RepositoryAction) -> eyre::Result<()> {
        self.effects
            .commands
            .send(WorkerCommand::Repository(action))?;
        Ok(())
    }

    fn terminal(&mut self, action: TerminalAction) -> eyre::Result<ControlFlow<()>> {
        (self.terminal)(action)
    }
}

#[cfg(test)]
pub(super) mod fixture;

#[cfg(test)]
#[path = "effects.tests.rs"]
mod tests;
