//! Explore shares the repository watcher's lifecycle and event pipeline.
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::{path::Path, sync::Arc};

pub(super) type StateObserver = Arc<dyn Fn(Result<(), String>) + Send + Sync>;

#[derive(Default)]
pub(super) struct StateWatch(Option<RecommendedWatcher>);

impl StateWatch {
    pub(super) fn watch(&mut self, path: &Path, changed: &StateObserver) {
        match Self::start(path, changed.clone()) {
            Ok(watcher) => self.0 = Some(watcher),
            Err(error) => changed(Err(error.to_string())),
        }
    }

    fn start(path: &Path, changed: StateObserver) -> notify::Result<RecommendedWatcher> {
        let mut watcher = RecommendedWatcher::new(
            move |event: notify::Result<notify::Event>| {
                let event = match event {
                    Ok(event) => event,
                    Err(error) => {
                        changed(Err(error.to_string()));
                        return;
                    }
                };
                let relevant = changes_a_round(&event);
                if relevant {
                    changed(Ok(()));
                }
            },
            notify::Config::default(),
        )?;
        watcher.watch(path, RecursiveMode::Recursive)?;
        Ok(watcher)
    }
}

/// Whether `event` changes a round: a JSON record other than an editor view or run-ahead's
/// forks, or a new directory.
fn changes_a_round(event: &notify::Event) -> bool {
    !matches!(event.kind, notify::EventKind::Access(_))
        && event.paths.iter().any(|path| {
            (path.extension().is_some_and(|ext| ext == "json")
                || matches!(
                    event.kind,
                    notify::EventKind::Create(notify::event::CreateKind::Folder)
                ))
                // Editor views and run-ahead's forks change no round.
                && ![b".view.json".as_slice(), b".forks.json"]
                    .iter()
                    .any(|suffix| path.as_os_str().as_encoded_bytes().ends_with(suffix))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, sync::mpsc};

    #[test]
    fn an_atomic_domain_update_notifies() {
        let root = tempfile::tempdir().unwrap();
        let (sent, received) = mpsc::channel();
        let observer: StateObserver = Arc::new(move |event| {
            let _ = sent.send(event);
        });
        let _watcher = StateWatch::start(root.path(), observer).unwrap();
        fs::write(root.path().join("round.json.new"), b"{}").unwrap();
        fs::rename(
            root.path().join("round.json.new"),
            root.path().join("round.json"),
        )
        .unwrap();
        // A guard: the notification ends the wait.
        received
            .recv_timeout(review_test_support::GUARD)
            .unwrap()
            .unwrap();
    }

    /// The events of renaming `from` to `to` in `root`, as inotify reports them.
    fn renamed(root: &Path, from: &str, to: &str) -> [notify::Event; 2] {
        use notify::event::{ModifyKind, RenameMode};
        let event = |mode, name: &str| {
            notify::Event::new(notify::EventKind::Modify(ModifyKind::Name(mode)))
                .add_path(root.join(name))
        };
        [event(RenameMode::From, from), event(RenameMode::To, to)]
    }

    #[test]
    fn domain_records_change_a_round_but_editor_autosaves_and_forks_do_not() {
        let root = Path::new("/state");
        assert!(
            renamed(root, "round.json.new", "round.json")
                .iter()
                .any(changes_a_round)
        );
        for record in ["round.view.json", "round.forks.json"] {
            assert!(
                !renamed(root, &format!("{record}.new"), record)
                    .iter()
                    .any(changes_a_round),
                "{record}"
            );
        }
        let read = notify::Event::new(notify::EventKind::Access(notify::event::AccessKind::Any))
            .add_path(root.join("round.json"));
        assert!(!changes_a_round(&read));
        let folder =
            notify::Event::new(notify::EventKind::Create(notify::event::CreateKind::Folder))
                .add_path(root.join("review"));
        assert!(changes_a_round(&folder));
    }
}
