//! Filesystem locations that signal a new repository snapshot.

use std::fs;
use std::path::{Path, PathBuf};

/// What a filesystem watcher observes to notice that a repository snapshot may have changed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WatchPlan {
    /// The working tree whose files and `.gitignore` rules define the review.
    pub root: PathBuf,
    /// Repository metadata whose changes signal a new repository state.
    pub metadata: Vec<MetadataWatch>,
    /// Git's global and repository exclude files, when the backend hides the paths they list.
    ///
    /// `None` means only `.gitignore` files inside the working tree hide paths.
    pub git_excludes: Option<Vec<PathBuf>>,
}

/// One repository metadata directory to watch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetadataWatch {
    /// The canonical directory path.
    pub directory: PathBuf,
    /// Which changes under the directory count.
    pub scope: MetadataScope,
}

/// Which changes under a watched metadata directory signal a new repository state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MetadataScope {
    /// Only entries directly inside the directory.
    Entries,
    /// Anything below the directory.
    Subtree,
}

impl MetadataWatch {
    /// Whether a change at `path` falls within this watch.
    pub fn includes(&self, path: &Path) -> bool {
        match self.scope {
            MetadataScope::Entries => path.parent() == Some(self.directory.as_path()),
            MetadataScope::Subtree => path.starts_with(&self.directory),
        }
    }
}

impl WatchPlan {
    pub(super) fn new(root: &Path) -> Self {
        Self {
            root: root.to_owned(),
            metadata: Vec::new(),
            git_excludes: None,
        }
    }

    /// Watch one metadata directory once, if it exists.
    pub(super) fn add_metadata(&mut self, directory: &Path, scope: MetadataScope) {
        let Ok(directory) = fs::canonicalize(directory) else {
            return;
        };
        if !self
            .metadata
            .iter()
            .any(|target| target.directory == directory)
        {
            self.metadata.push(MetadataWatch { directory, scope });
        }
    }
}

/// Resolve a directory, or a file that holds the path of one relative to its parent.
///
/// Both jj (`.jj/repo`) and Git (`commondir`) use such link files for shared metadata.
pub(super) fn resolve_directory_or_link_file(path: &Path) -> Option<PathBuf> {
    if path.is_dir() {
        return fs::canonicalize(path).ok();
    }
    let value = fs::read_to_string(path).ok()?;
    fs::canonicalize(path.parent()?.join(value.trim())).ok()
}
