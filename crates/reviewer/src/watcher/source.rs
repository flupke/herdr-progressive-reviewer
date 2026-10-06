//! Watch the displayed source even when ignore rules exclude its directory.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use notify::{Event, RecommendedWatcher, RecursiveMode};

use super::{WatchCommand, WatchState, should_process, watch_present};

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
        state.notify(&[]);
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
        state.notify(&event.paths);
        if event
            .paths
            .iter()
            .any(|changed| changed != path && path.starts_with(changed))
        {
            // An ancestor disappeared or was recreated; follow the new directory
            // identities without watching unrelated ignored subtrees.
            let path = path.clone();
            self.install()?;
            // What changed between this event and the new watches, they did not see.
            state.notify(&[path]);
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
        // recreated parents, including when the source is initially absent. They start from
        // the root down: a parent that goes and comes back meanwhile, its parent's watch sees.
        let mut parents = Vec::new();
        for parent in path.ancestors().skip(1) {
            parents.push(parent);
            if parent == self.root {
                break;
            }
        }
        for parent in parents.into_iter().rev() {
            if parent.is_dir() {
                watch_present(&mut watcher, parent, RecursiveMode::NonRecursive)?;
            }
        }
        self.watcher = Some(watcher);
        Ok(())
    }
}
