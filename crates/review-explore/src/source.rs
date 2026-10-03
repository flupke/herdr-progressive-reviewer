use std::{
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

use review_repository::repository::{
    ChangedFile, RepoPath, Repository, Snapshot, SnapshotIdentity,
};
use review_source::SourceLineRange;
use serde::{Deserialize, Serialize};

#[derive(
    Clone,
    Copy,
    Debug,
    Deserialize,
    Serialize,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum SourceSide {
    Old,
    New,
}

impl SourceSide {
    /// `old` on the old side, `new` on the new side.
    pub(crate) fn pick<T>(self, old: T, new: T) -> T {
        match self {
            Self::Old => old,
            Self::New => new,
        }
    }

    /// The path of `file` on this side, when the file exists there.
    pub(crate) fn path_in(self, file: &ChangedFile) -> Option<&RepoPath> {
        self.pick(file.old_path.as_ref(), file.new_path.as_ref())
    }
}

/// `old` or `new`, as citations name the sides.
impl std::fmt::Display for SourceSide {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        output.write_str(match self {
            Self::Old => "old",
            Self::New => "new",
        })
    }
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
    pub lines: Option<SourceLineRange>,
}

/// `path new 7-9`, or `path (whole file)`.
impl std::fmt::Display for CodeLocation {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.lines {
            Some(lines) => write!(output, "{} {} {lines}", self.path.display(), self.side),
            None => write!(output, "{} (whole file)", self.path.display()),
        }
    }
}

impl CodeLocation {
    /// The one-based location of zero-based `lines` on one side of `file`.
    pub fn on(file: &ChangedFile, side: SourceSide, lines: std::ops::Range<u32>) -> Self {
        let path = match side {
            SourceSide::Old => file.old_path.as_ref(),
            SourceSide::New => file.new_path.as_ref(),
        }
        .unwrap_or_else(|| file.review_path());
        Self {
            path: path.clone(),
            side,
            lines: Some(SourceLineRange::from_zero_based(lines)),
        }
    }

    /// Whether the location is in `file`: its path on the location's side,
    /// or either of its paths for the whole file.
    pub fn names(&self, file: &ChangedFile) -> bool {
        let old = file.old_path.as_ref() == Some(&self.path);
        let new = file.new_path.as_ref() == Some(&self.path);
        match (&self.lines, self.side) {
            (None, _) => old || new,
            (Some(_), SourceSide::Old) => old,
            (Some(_), SourceSide::New) => new,
        }
    }

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
    /// Why this source matters to the question, as a short paragraph with optional detail.
    pub notes: String,
}
