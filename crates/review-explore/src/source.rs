use std::{
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

use review_guide::GuideLineRange;
use review_repository::repository::{RepoPath, Repository, Snapshot, SnapshotIdentity};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, Hash, PartialEq, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourceSide {
    Old,
    New,
}

/// A citation supplied by the agent, without a reviewer-assigned source ID.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq, schemars::JsonSchema)]
pub struct CodeLocation {
    /// Repository-relative UTF-8 path or lossless raw path bytes.
    #[serde(with = "crate::path_serde")]
    #[schemars(with = "crate::path_serde::PathInput")]
    pub path: RepoPath,
    pub side: SourceSide,
    /// One-based inclusive lines. None identifies the whole file.
    pub lines: Option<GuideLineRange>,
}

impl CodeLocation {
    pub(crate) fn relative_path(&self) -> Option<&Path> {
        let path = Path::new(std::ffi::OsStr::from_bytes(self.path.as_bytes()));
        (!path.as_os_str().is_empty()
            && path
                .components()
                .all(|part| matches!(part, std::path::Component::Normal(_))))
        .then_some(path)
    }
}

/// Source content for native evidence viewers. Working-copy text is read on demand.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Source {
    pub path: RepoPath,
    pub display_path: String,
    pub side: SourceSide,
    pub content: Option<Vec<u8>>,
    pub limitation: Option<String>,
    pub base: Option<SnapshotIdentity>,
}

impl Source {
    pub fn read(&self, root: &Path) -> eyre::Result<Vec<u8>> {
        if self.side == SourceSide::Old {
            if let Some(content) = &self.content {
                return Ok(content.clone());
            }
            eyre::ensure!(
                self.limitation.is_none(),
                "{}",
                self.limitation.as_deref().unwrap_or_default()
            );
            let identity = self
                .base
                .clone()
                .ok_or_else(|| eyre::eyre!("Base source unavailable"))?;
            return Ok(Repository::discover(root)?.base_file_at(
                &Snapshot {
                    identity,
                    files: Vec::new(),
                },
                &self.path,
            )?);
        }
        Ok(std::fs::read(self.disk_path(root)?)?)
    }

    pub(crate) fn disk_path(&self, root: &Path) -> eyre::Result<PathBuf> {
        let path = root.join(std::ffi::OsStr::from_bytes(self.path.as_bytes()));
        eyre::ensure!(
            std::fs::symlink_metadata(&path)?.is_file(),
            "Source is not a regular working-copy file"
        );
        let path = path.canonicalize()?;
        eyre::ensure!(
            path.starts_with(root.canonicalize()?),
            "Source is outside the repository"
        );
        Ok(path)
    }

    pub fn read_text(&self, root: &Path) -> eyre::Result<String> {
        let bytes = self.read(root)?;
        eyre::ensure!(
            !bytes.contains(&0),
            "Non-text source; use file-level evidence"
        );
        Ok(String::from_utf8(bytes)?)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq, schemars::JsonSchema)]
pub struct EvidenceRef {
    #[serde(flatten)]
    pub location: CodeLocation,
    /// What the cited source establishes.
    pub relationship: String,
    /// Why this snippet could change the answer to the displayed question.
    #[serde(default)]
    pub decision_relevance: String,
}
