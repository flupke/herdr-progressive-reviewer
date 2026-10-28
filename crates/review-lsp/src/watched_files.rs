use std::collections::HashMap;
use std::path::{Path, PathBuf};

use globset::{GlobBuilder, GlobMatcher};
use lsp_types::{
    DidChangeWatchedFilesRegistrationOptions, FileChangeType, FileEvent, GlobPattern, OneOf,
    Registration, Unregistration, WatchKind,
};

use crate::api::{FileChange, FileChangeKind};
use crate::source::{changed_path_uri, uri_path};

const METHOD: &str = "workspace/didChangeWatchedFiles";

/// The files a language server asked to hear about through `workspace/didChangeWatchedFiles`.
///
/// Expert registers its watchers whether or not the client offers the capability, and relies
/// on them to index the files that change after its first build.
pub(super) struct WatchedFiles {
    root: PathBuf,
    registrations: HashMap<String, Vec<Watcher>>,
}

struct Watcher {
    /// The directory a relative pattern matches from; `None` for a plain pattern.
    base: Option<PathBuf>,
    glob: GlobMatcher,
    kind: WatchKind,
}

impl WatchedFiles {
    pub(super) fn new(root: &Path) -> Self {
        Self {
            root: root.to_owned(),
            registrations: HashMap::new(),
        }
    }

    pub(super) fn register(&mut self, registration: Registration) {
        if registration.method != METHOD {
            return;
        }
        let Some(options) = registration.register_options.and_then(|options| {
            serde_json::from_value::<DidChangeWatchedFilesRegistrationOptions>(options).ok()
        }) else {
            return;
        };
        let watchers = options
            .watchers
            .into_iter()
            .filter_map(|watcher| {
                let (base, pattern) = match watcher.glob_pattern {
                    GlobPattern::String(pattern) => (None, pattern),
                    GlobPattern::Relative(relative) => {
                        let base = match relative.base_uri {
                            OneOf::Left(folder) => folder.uri,
                            OneOf::Right(uri) => uri,
                        };
                        (Some(uri_path(&base)?), relative.pattern)
                    }
                };
                let glob = GlobBuilder::new(&pattern)
                    .literal_separator(true)
                    .build()
                    .ok()?
                    .compile_matcher();
                Some(Watcher {
                    base,
                    glob,
                    kind: watcher.kind.unwrap_or(WatchKind::all()),
                })
            })
            .collect();
        self.registrations.insert(registration.id, watchers);
    }

    pub(super) fn unregister(&mut self, unregistration: &Unregistration) {
        if unregistration.method == METHOD {
            self.registrations.remove(&unregistration.id);
        }
    }

    /// The events to send for `changes`: those a registered watcher matches.
    pub(super) fn events(&self, changes: &[FileChange]) -> Vec<FileEvent> {
        changes
            .iter()
            .filter(|change| self.watches(change))
            .filter_map(|change| {
                Some(FileEvent::new(
                    changed_path_uri(&change.path).ok()?,
                    lsp_kinds(change.kind).1,
                ))
            })
            .collect()
    }

    fn watches(&self, change: &FileChange) -> bool {
        let kind = lsp_kinds(change.kind).0;
        self.registrations
            .values()
            .flatten()
            .filter(|watcher| watcher.kind.contains(kind))
            .any(|watcher| match &watcher.base {
                Some(base) => change
                    .path
                    .strip_prefix(base)
                    .is_ok_and(|path| watcher.glob.is_match(path)),
                // A plain pattern such as `**/*.ex` matches the absolute path; one such as
                // `lib/**` matches from the project root.
                None => {
                    watcher.glob.is_match(&change.path)
                        || change
                            .path
                            .strip_prefix(&self.root)
                            .is_ok_and(|path| watcher.glob.is_match(path))
                }
            })
    }
}

/// The watch kind that hears a change of `kind`, and the type of its event.
fn lsp_kinds(kind: FileChangeKind) -> (WatchKind, FileChangeType) {
    match kind {
        FileChangeKind::Created => (WatchKind::Create, FileChangeType::CREATED),
        FileChangeKind::Changed => (WatchKind::Change, FileChangeType::CHANGED),
        FileChangeKind::Deleted => (WatchKind::Delete, FileChangeType::DELETED),
    }
}

#[cfg(test)]
#[path = "watched_files.tests.rs"]
mod tests;
