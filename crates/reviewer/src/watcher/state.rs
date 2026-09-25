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
                let relevant = {
                    !matches!(event.kind, notify::EventKind::Access(_))
                        && event.paths.iter().any(|path| {
                            (path.extension().is_some_and(|ext| ext == "json")
                                || matches!(
                                    event.kind,
                                    notify::EventKind::Create(notify::event::CreateKind::Folder)
                                ))
                                && !path.as_os_str().as_encoded_bytes().ends_with(b".view.json")
                        })
                };
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, sync::mpsc, time::Duration};

    #[test]
    fn atomic_domain_updates_notify_but_editor_autosaves_do_not() {
        let root = tempfile::tempdir().unwrap();
        let (sent, received) = mpsc::channel();
        let observer: StateObserver = Arc::new(move |event| {
            let _ = sent.send(event);
        });
        let _watcher = StateWatch::start(root.path(), observer).unwrap();
        fs::write(root.path().join("pass.json.new"), b"{}").unwrap();
        fs::rename(
            root.path().join("pass.json.new"),
            root.path().join("pass.json"),
        )
        .unwrap();
        received
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .unwrap();
        // Drain the finite notification burst from the atomic domain write.
        while received.recv_timeout(Duration::from_millis(100)).is_ok() {}
        fs::write(root.path().join("pass.view.json.new"), b"{}").unwrap();
        fs::rename(
            root.path().join("pass.view.json.new"),
            root.path().join("pass.view.json"),
        )
        .unwrap();
        assert!(matches!(
            received.recv_timeout(Duration::from_millis(200)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
    }
}
