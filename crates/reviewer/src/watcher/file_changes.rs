//! The files that changed on disk, for the language servers.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use notify::event::ModifyKind;
use notify::{Event, EventKind};
use review_lsp::{FileChange, FileChangeKind, FileChangeSender};

/// The files that changed since the language servers last heard, sent with each refresh.
#[derive(Default)]
pub(super) struct FileChanges {
    /// Each changed path, and whether it appeared since.
    paths: Mutex<BTreeMap<PathBuf, bool>>,
}

impl FileChanges {
    /// Records the paths of `event` that `includes` keeps, given a path and whether it is a
    /// directory: the files of the working tree. A directory that appears, moved in or
    /// created, stands for the files it holds.
    pub(super) fn record(&self, event: &Event, includes: impl Fn(&Path, bool) -> bool) {
        let appeared = matches!(
            event.kind,
            EventKind::Create(_) | EventKind::Modify(ModifyKind::Name(_))
        );
        let mut recorded = Vec::new();
        for path in &event.paths {
            let is_dir = is_directory(path);
            if !includes(path, is_dir) {
                continue;
            }
            if appeared && is_dir {
                recorded.extend(files_under(path, &includes));
            } else {
                recorded.push(path.clone());
            }
        }
        let mut paths = self.paths.lock().unwrap_or_else(PoisonError::into_inner);
        for path in recorded {
            *paths.entry(path).or_default() |= appeared;
        }
    }

    /// Sends the changes recorded so far, each as the disk holds it now.
    pub(super) fn send(&self, sender: &FileChangeSender) {
        let changes = self.take();
        if !changes.is_empty() {
            sender.send(changes);
        }
    }

    pub(super) fn take(&self) -> Vec<FileChange> {
        let paths = std::mem::take(&mut *self.paths.lock().unwrap_or_else(PoisonError::into_inner));
        paths
            .into_iter()
            .filter(|(path, _)| !is_directory(path))
            .map(|(path, appeared)| {
                let kind = if !path.exists() {
                    FileChangeKind::Deleted
                } else if appeared {
                    FileChangeKind::Created
                } else {
                    FileChangeKind::Changed
                };
                FileChange { path, kind }
            })
            .collect()
    }
}

/// Whether `path` is a directory itself, not a link to one.
fn is_directory(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|metadata| metadata.is_dir())
}

/// The files under `directory` that `includes` keeps, in the directories it keeps. Links are
/// not followed: a link to a directory counts as a file, as the watcher sees it.
fn files_under(directory: &Path, includes: &impl Fn(&Path, bool) -> bool) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut directories = vec![directory.to_owned()];
    while let Some(directory) = directories.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let is_dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
            let path = entry.path();
            if !includes(&path, is_dir) {
                continue;
            }
            if is_dir {
                directories.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files
}

#[cfg(test)]
#[path = "file_changes.tests.rs"]
mod tests;
