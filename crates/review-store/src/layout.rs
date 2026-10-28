//! Where the review state of one repository lives: one folder that every jj workspace, or
//! every Git worktree, of the repository shares, named so a reader finds the repository.
//!
//! The folder is `repositories/<slug>-<hash>`: `<hash>` is the first 16 hex digits of the
//! SHA-256 of the canonical path of the store the workspaces share (`.jj/repo` resolved, or the
//! Git common directory), and `<slug>` the name of the parent worktree. `repository.json` in
//! the folder holds that path in full (ADR 0013).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{Error, Result, ReviewStore, StateKey};

/// The folder under the state root that holds every repository's state.
pub(super) const REPOSITORIES: &str = "repositories";
/// The file in a repository's folder that names the repository.
const REPOSITORY_RECORD: &str = "repository.json";
/// How many hex digits of the store path's hash a folder name keeps.
const HASH_DIGITS: usize = 16;
/// How many bytes of the parent worktree's name a folder name keeps.
const SLUG_BYTES: usize = 64;

/// What a repository's folder says about the repository.
#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
struct RepositoryRecord {
    version: u8,
    kind: String,
    store: String,
}

/// The shared store of a repository, by its canonical path.
pub(super) struct SharedStore {
    path: String,
}

impl SharedStore {
    pub(super) fn canonical(store: &Path) -> Result<Self> {
        let canonical = std::fs::canonicalize(store).map_err(|source| Error::StateIo {
            operation: "canonicalize repository store",
            path: store.to_owned(),
            source,
        })?;
        let path =
            canonical
                .into_os_string()
                .into_string()
                .map_err(|_| Error::InvalidStateKey {
                    field: "repository store",
                })?;
        Ok(Self { path })
    }

    /// The folder of the repository's state, under `state_root`.
    pub(super) fn directory(&self, state_root: &Path) -> PathBuf {
        let hash = StateKey::hash(self.path.as_bytes()).0;
        state_root
            .join(REPOSITORIES)
            .join(format!("{}-{}", self.slug(), &hash[..HASH_DIGITS]))
    }

    /// `jj` for a jj store, which is a `.jj/repo` directory, `git` for any other.
    fn kind(&self) -> &'static str {
        if Path::new(&self.path).ends_with(".jj/repo") {
            "jj"
        } else {
            "git"
        }
    }

    /// The name of the parent worktree: the jj workspace that holds `.jj/repo`, the Git
    /// worktree that holds the common `.git`, or a bare Git repository's own directory. Every
    /// byte outside `[A-Za-z0-9._-]` becomes `_`.
    fn slug(&self) -> String {
        let store = Path::new(&self.path);
        let parent = if self.kind() == "jj" {
            store.parent().and_then(Path::parent)
        } else if store.file_name().is_some_and(|name| name == ".git") {
            store.parent()
        } else {
            Some(store)
        };
        let name = parent
            .and_then(Path::file_name)
            .map_or(b"repository".as_slice(), |name| name.as_encoded_bytes());
        name.iter()
            .take(SLUG_BYTES)
            .map(|&byte| {
                if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-') {
                    char::from(byte)
                } else {
                    '_'
                }
            })
            .collect()
    }

    fn record(&self) -> RepositoryRecord {
        RepositoryRecord {
            version: 1,
            kind: self.kind().to_owned(),
            store: self.path.clone(),
        }
    }
}

impl ReviewStore {
    /// Refuse the folder when its `repository.json` names another repository, which only a
    /// collision of the short hash gives.
    pub(super) fn check_repository_record(&self, store: &SharedStore) -> Result<()> {
        let path = self.repository_dir.join(REPOSITORY_RECORD);
        match Self::read_json::<RepositoryRecord>(&path, "read repository record")? {
            Some(record) if record.store != store.path => Err(Error::StateCollision { path }),
            _ => Ok(()),
        }
    }

    /// Write `repository.json` once, before any record of the folder.
    pub(super) fn write_repository_record(&self, store: &SharedStore) -> Result<()> {
        let path = self.repository_dir.join(REPOSITORY_RECORD);
        if path.exists() {
            return Ok(());
        }
        self.atomic_json(&path, &store.record(), "write repository record")
    }
}

#[cfg(test)]
#[path = "layout.tests.rs"]
mod tests;
