//! Watch the displayed source even when ignore rules exclude its directory.

use std::path::{Path, PathBuf};
use std::sync::{atomic::Ordering, mpsc::Sender};

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

use super::{WatchCommand, WatchState, should_process};

pub(super) struct SourceWatch {
    root: PathBuf,
    commands: Sender<WatchCommand>,
    path: Option<PathBuf>,
    watcher: Option<RecommendedWatcher>,
}

impl SourceWatch {
    pub(super) fn new(root: PathBuf, commands: Sender<WatchCommand>) -> Self {
        Self {
            root,
            commands,
            path: None,
            watcher: None,
        }
    }

    pub(super) fn clear(&mut self) {
        self.path = None;
        self.watcher = None;
    }

    pub(super) fn select(&mut self, path: PathBuf, state: &WatchState) -> notify::Result<()> {
        if self.path.as_ref() == Some(&path) {
            return Ok(());
        }
        self.path = Some(path);
        self.install()?;
        // Close the gap between the first read and installing these watches.
        state.notified.store(true, Ordering::Relaxed);
        Ok(())
    }

    pub(super) fn update(&mut self, event: &Event, state: &WatchState) -> notify::Result<()> {
        if event.need_rescan() {
            return Err(notify::Error::generic("source filesystem events were lost"));
        }
        let Some(path) = &self.path else {
            return Ok(());
        };
        if !Self::includes(path, event) {
            return Ok(());
        }
        state.notified.store(true, Ordering::Relaxed);
        if event
            .paths
            .iter()
            .any(|changed| changed != path && path.starts_with(changed))
        {
            // An ancestor disappeared or was recreated; follow the new directory
            // identities without watching unrelated ignored subtrees.
            self.install()?;
        }
        Ok(())
    }

    fn includes(path: &Path, event: &Event) -> bool {
        event.paths.iter().any(|changed| path.starts_with(changed))
    }

    fn install(&mut self) -> notify::Result<()> {
        let path = self.path.as_ref().expect("selected source");
        let source_path = path.clone();
        let commands = self.commands.clone();
        let mut watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
            let command = match event {
                Ok(event)
                    if event.need_rescan()
                        || (should_process(&event) && Self::includes(&source_path, &event)) =>
                {
                    WatchCommand::SourceEvent(event)
                }
                Ok(_) => return,
                Err(_) => WatchCommand::Failed,
            };
            let _ = commands.send(command);
        })?;
        // Parent watches survive atomic replacement. Ancestor watches discover
        // recreated parents, including when the source is initially absent.
        for parent in path.ancestors().skip(1) {
            if parent.is_dir() {
                watcher.watch(parent, RecursiveMode::NonRecursive)?;
            }
            if parent == self.root {
                break;
            }
        }
        self.watcher = Some(watcher);
        Ok(())
    }
}
