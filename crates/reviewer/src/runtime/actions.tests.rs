use super::*;
use review_source::ReviewCheckpoint;
use review_threads::ThreadCommand;
use review_ui::DocumentLoad;
use std::path::PathBuf;

/// Records which executor received each action, rewrapped as the action it ran.
#[derive(Default)]
struct Recorder {
    runs: Vec<(&'static str, Action)>,
    fail_settings: bool,
}

impl Recorder {
    fn record(&mut self, executor: &'static str, action: Action) {
        self.runs.push((executor, action));
    }

    fn executors(&self) -> Vec<&'static str> {
        self.runs.iter().map(|(executor, _)| *executor).collect()
    }
}

impl ActionExecutors for Recorder {
    fn explore(&mut self, command: review_explore::Command) -> eyre::Result<()> {
        self.record("explore", Action::Explore(command));
        Ok(())
    }

    fn explore_page(&mut self, action: ExplorePageAction) -> eyre::Result<()> {
        self.record("explore page", Action::ExplorePage(action));
        Ok(())
    }

    fn thread(&mut self, command: ThreadCommand) -> eyre::Result<()> {
        self.record("thread", Action::Thread(command));
        Ok(())
    }

    fn document(&mut self, action: DocumentAction) -> eyre::Result<()> {
        self.record("document", Action::Document(action));
        Ok(())
    }

    fn lsp(&mut self, action: LspAction) -> eyre::Result<()> {
        self.record("lsp", Action::Lsp(action));
        Ok(())
    }

    fn settings(&mut self, action: SettingsAction) -> eyre::Result<()> {
        if self.fail_settings {
            eyre::bail!("settings are read-only");
        }
        self.record("settings", Action::Settings(action));
        Ok(())
    }

    fn repository(&mut self, action: RepositoryAction) -> eyre::Result<()> {
        self.record("repository", Action::Repository(action));
        Ok(())
    }

    fn terminal(&mut self, action: TerminalAction) -> eyre::Result<ControlFlow<()>> {
        let flow = match action {
            TerminalAction::Quit => ControlFlow::Break(()),
            TerminalAction::OpenInEditor { .. } => ControlFlow::Continue(()),
        };
        self.record("terminal", Action::Terminal(action));
        Ok(flow)
    }
}

fn checkpoint() -> ReviewCheckpoint {
    ReviewCheckpoint::new("change", "checkpoint")
}

#[test]
fn each_action_group_reaches_its_own_executor_with_its_payload() {
    let mut recorder = Recorder::default();
    let actions = vec![
        Action::Explore(review_explore::Command::Cancel),
        Action::ExplorePage(ExplorePageAction::Open),
        Action::Thread(ThreadCommand::Load("change".into())),
        Action::Document(DocumentAction::Load(DocumentLoad::Diff {
            review_checkpoint: checkpoint(),
            path: "src/lib.rs".into(),
        })),
        Action::Lsp(LspAction::Restart),
        Action::Settings(SettingsAction::SaveFilePaneWidth(42)),
        Action::Repository(RepositoryAction::UnreviewAll(checkpoint())),
        Action::Terminal(TerminalAction::OpenInEditor {
            path: PathBuf::from("src/lib.rs"),
            line: Some(3),
        }),
    ];

    let flow = recorder.run_all(actions.clone()).unwrap();

    assert_eq!(flow, ControlFlow::Continue(()));
    let executors = [
        "explore",
        "explore page",
        "thread",
        "document",
        "lsp",
        "settings",
        "repository",
        "terminal",
    ];
    let expected = executors.into_iter().zip(actions).collect::<Vec<_>>();
    assert_eq!(recorder.runs, expected);
}

#[test]
fn quit_stops_before_later_actions_run() {
    let mut recorder = Recorder::default();

    let flow = recorder
        .run_all(vec![
            Action::Settings(SettingsAction::SaveFilePaneWidth(42)),
            Action::Terminal(TerminalAction::Quit),
            Action::Lsp(LspAction::Restart),
        ])
        .unwrap();

    assert_eq!(flow, ControlFlow::Break(()));
    assert_eq!(recorder.executors(), ["settings", "terminal"]);
}

#[test]
fn an_executor_failure_stops_routing_and_is_returned() {
    let mut recorder = Recorder {
        fail_settings: true,
        ..Recorder::default()
    };

    let error = recorder
        .run_all(vec![
            Action::Lsp(LspAction::Restart),
            Action::Settings(SettingsAction::SaveFilePaneWidth(42)),
            Action::Repository(RepositoryAction::AutoReview(checkpoint())),
        ])
        .unwrap_err();

    assert_eq!(error.to_string(), "settings are read-only");
    assert_eq!(recorder.executors(), ["lsp"]);
}
