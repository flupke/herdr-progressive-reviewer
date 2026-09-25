//! Stable snapshots of one jj change or Git working tree.

use std::collections::{BTreeMap, HashMap};
use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

use crate::{Error, Result};
use review_types::ReviewUnit;

mod comparison_plan;
mod git;
mod git_path;
mod jj;
mod jj_git_diff;
mod jj_patch;
mod jj_reader;

pub use comparison_plan::{BaselineComparison, BaselineComparisonPlan, BaselineComparisonResults};

const COMMAND_OUTPUT_LIMIT: usize = 256 * 1024 * 1024;

#[derive(Clone, Copy, Debug)]
struct RepositoryProcess<'a> {
    program: &'static str,
    cwd: &'a Path,
    operation: &'static str,
    cancellation: &'a Cancellation,
    environment: &'a [(OsString, OsString)],
}

#[derive(Clone, Debug, Default)]
struct Cancellation(Arc<AtomicBool>);

impl Cancellation {
    fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// A full stable jj change identifier.
#[derive(Clone, Debug, Eq, Hash, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ChangeId(ReviewUnit);

impl ChangeId {
    /// Get the identifier text.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Get the review unit identified by this jj change.
    pub fn review_unit(&self) -> &ReviewUnit {
        &self.0
    }
}

impl From<String> for ChangeId {
    fn from(value: String) -> Self {
        Self(value.into())
    }
}

impl From<&ReviewUnit> for ChangeId {
    fn from(value: &ReviewUnit) -> Self {
        Self(value.clone())
    }
}

/// An exact repository snapshot identifier.
#[derive(
    Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize,
)]
pub struct SnapshotId(String);

impl SnapshotId {
    /// Get the identifier text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for SnapshotId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

/// A lossless Unix repository-relative path.
#[derive(
    Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize,
)]
pub struct RepoPath(Vec<u8>);

impl RepoPath {
    pub fn from_bytes(bytes: impl Into<Vec<u8>>) -> Self {
        Self(bytes.into())
    }

    /// Get the lossless path bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    fn as_os_str(&self) -> &OsStr {
        OsStr::from_bytes(&self.0)
    }

    /// Return readable UTF-8 while preserving escaped ASCII, controls, and invalid bytes.
    pub fn display(&self) -> String {
        match std::str::from_utf8(&self.0) {
            Ok(path) => {
                let mut display = String::with_capacity(path.len());
                for character in path.chars() {
                    if character.is_ascii() {
                        display.extend(std::ascii::escape_default(character as u8).map(char::from));
                    } else if character.is_control() {
                        display.extend(character.escape_default());
                    } else {
                        display.push(character);
                    }
                }
                display
            }
            Err(_) => self
                .0
                .iter()
                .flat_map(|byte| std::ascii::escape_default(*byte))
                .map(char::from)
                .collect(),
        }
    }
}

/// The repository entry type on one side of a change.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum FileKind {
    /// No entry exists on this side.
    Absent,
    /// A regular file exists.
    File,
    /// A symbolic link exists.
    Symlink,
    /// The entry contains a merge conflict.
    Conflict,
    /// The entry is a Git submodule.
    Gitlink,
}

impl FileKind {
    fn parse(value: &[u8]) -> Result<Self> {
        match value {
            b"" => Ok(Self::Absent),
            b"file" => Ok(Self::File),
            b"symlink" => Ok(Self::Symlink),
            b"conflict" => Ok(Self::Conflict),
            _ => Err(Error::Protocol {
                operation: "read jj changed files".to_owned(),
                detail: "jj returned an unknown file type",
            }),
        }
    }

    fn parse_git_mode(value: &[u8]) -> Result<Self> {
        match value {
            b"000000" => Ok(Self::Absent),
            b"100644" | b"100755" => Ok(Self::File),
            b"120000" => Ok(Self::Symlink),
            b"160000" => Ok(Self::Gitlink),
            _ => Err(Error::Protocol {
                operation: "read Git changed files".to_owned(),
                detail: "Git returned an unknown file mode",
            }),
        }
    }
}

/// The normalized change for one file row.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ChangeKind {
    /// A path was added.
    Added,
    /// File content or executable state changed.
    Modified,
    /// A path was deleted.
    Deleted,
    /// A path was renamed.
    Renamed,
    /// The entry type changed.
    TypeChanged,
    /// The entry contains a merge conflict.
    Conflict,
}

/// One changed path in the current review.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ChangedFile {
    /// The path on the parent side.
    pub old_path: Option<RepoPath>,
    /// The path on the current side.
    pub new_path: Option<RepoPath>,
    /// The entry type on the parent side.
    pub old_kind: FileKind,
    /// The entry type on the current side.
    pub new_kind: FileKind,
    /// The normalized change type.
    pub change: ChangeKind,
    /// Escaped text for the file list.
    pub display_path: String,
    /// Statistics for the complete change under review.
    pub statistics: DiffStatistics,
}

/// Added and removed text lines in one diff.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DiffStatistics {
    /// Number of added text lines.
    pub lines_added: u64,
    /// Number of removed text lines.
    pub lines_removed: u64,
}

impl DiffStatistics {
    /// Count changed text lines in a unified diff.
    pub fn from_unified_diff(diff: &[u8]) -> Self {
        let mut statistics = Self::default();
        let mut in_hunk = false;
        for line in diff.split(|byte| *byte == b'\n') {
            if line.starts_with(b"diff --git ") {
                in_hunk = false;
            } else if line.starts_with(b"@@ ") {
                in_hunk = true;
            } else if in_hunk && line.starts_with(b"+") {
                statistics.lines_added += 1;
            } else if in_hunk && line.starts_with(b"-") {
                statistics.lines_removed += 1;
            }
        }
        statistics
    }
}

#[derive(Clone, Copy, Debug)]
struct GitChangeMetadata<'a> {
    old_kind: FileKind,
    new_kind: FileKind,
    status: &'a [u8],
}

impl<'a> GitChangeMetadata<'a> {
    fn parse(metadata: &'a [u8]) -> Result<Self> {
        let mut values = metadata.split(|byte| *byte == b' ');
        let old_mode = values.next().and_then(|value| value.strip_prefix(b":"));
        let new_mode = values.next();
        let _old_object = values.next();
        let _new_object = values.next();
        let status = values.next();
        if values.next().is_some() {
            return Err(Error::Protocol {
                operation: "read Git changed files".to_owned(),
                detail: "Git returned invalid raw diff metadata",
            });
        }
        let (Some(old_mode), Some(new_mode), Some(status)) = (old_mode, new_mode, status) else {
            return Err(Error::Protocol {
                operation: "read Git changed files".to_owned(),
                detail: "Git returned incomplete raw diff metadata",
            });
        };
        Ok(Self {
            old_kind: FileKind::parse_git_mode(old_mode)?,
            new_kind: FileKind::parse_git_mode(new_mode)?,
            status,
        })
    }

    fn second_path(self, fields: &mut impl Iterator<Item = &'a [u8]>) -> Result<Option<&'a [u8]>> {
        if !matches!(self.status.first(), Some(b'R' | b'C')) {
            return Ok(None);
        }
        fields.next().map(Some).ok_or_else(|| Error::Protocol {
            operation: "read Git changed files".to_owned(),
            detail: "Git returned a rename without its new path",
        })
    }

    fn change(self) -> Result<ChangeKind> {
        if self.status.first() == Some(&b'U') {
            return Ok(ChangeKind::Conflict);
        }
        if ChangedFile::entry_type_changed(self.old_kind, self.new_kind) {
            return Ok(ChangeKind::TypeChanged);
        }
        match self.status.first() {
            Some(b'A' | b'C') => Ok(ChangeKind::Added),
            Some(b'M') => Ok(ChangeKind::Modified),
            Some(b'D') => Ok(ChangeKind::Deleted),
            Some(b'R') => Ok(ChangeKind::Renamed),
            Some(b'T') => Ok(ChangeKind::TypeChanged),
            _ => Err(Error::Protocol {
                operation: "read Git changed files".to_owned(),
                detail: "Git returned an unknown file status",
            }),
        }
    }
}

impl ChangedFile {
    /// Create metadata for a modified repository-relative path.
    pub fn modified(path: impl AsRef<OsStr>) -> Self {
        let path = RepoPath::from_bytes(path.as_ref().as_bytes());
        Self {
            old_path: Some(path.clone()),
            new_path: Some(path.clone()),
            old_kind: FileKind::File,
            new_kind: FileKind::File,
            change: ChangeKind::Modified,
            display_path: path.display(),
            statistics: DiffStatistics::default(),
        }
    }

    /// Get the path used for review state.
    ///
    /// # Panics
    ///
    /// Panics if both path fields are `None`.
    pub fn review_path(&self) -> &RepoPath {
        self.new_path
            .as_ref()
            .or(self.old_path.as_ref())
            .expect("a changed file always has a path")
    }

    fn parse_all(output: &[u8]) -> Result<Vec<Self>> {
        if output.is_empty() {
            return Ok(Vec::new());
        }
        let fields: Vec<_> = output.split(|byte| *byte == 0).collect();
        if fields.last() != Some(&&[][..]) || (fields.len() - 1) % 5 != 0 {
            return Err(Error::Protocol {
                operation: "read jj changed files".to_owned(),
                detail: "jj returned an invalid file record",
            });
        }

        let mut files = Vec::with_capacity((fields.len() - 1) / 5);
        for fields in fields[..fields.len() - 1].chunks_exact(5) {
            files.push(Self::parse(fields)?);
        }
        files.sort_by(|left, right| {
            left.review_path()
                .as_bytes()
                .cmp(right.review_path().as_bytes())
        });
        Ok(files)
    }

    fn parse(fields: &[&[u8]]) -> Result<Self> {
        let old_kind = FileKind::parse(fields[2])?;
        let new_kind = FileKind::parse(fields[3])?;
        let old_path = (old_kind != FileKind::Absent).then(|| RepoPath::from_bytes(fields[0]));
        let new_path = (new_kind != FileKind::Absent).then(|| RepoPath::from_bytes(fields[1]));
        let status = std::str::from_utf8(fields[4]).map_err(|_| Error::Protocol {
            operation: "read jj changed files".to_owned(),
            detail: "jj returned a non-UTF-8 file status",
        })?;

        let change = Self::jj_change(old_kind, new_kind, status)?;
        let display_path =
            Self::display_path(old_path.as_ref(), new_path.as_ref()).ok_or_else(|| {
                Error::Protocol {
                    operation: "read jj changed files".to_owned(),
                    detail: "jj returned a changed file without a path",
                }
            })?;

        Ok(Self {
            old_path,
            new_path,
            old_kind,
            new_kind,
            change,
            display_path,
            statistics: DiffStatistics::default(),
        })
    }

    fn display_path(old_path: Option<&RepoPath>, new_path: Option<&RepoPath>) -> Option<String> {
        match (old_path, new_path) {
            (Some(old), Some(new)) if old != new => {
                Some(format!("{} => {}", old.display(), new.display()))
            }
            (Some(path), _) | (_, Some(path)) => Some(path.display()),
            (None, None) => None,
        }
    }

    fn jj_change(old_kind: FileKind, new_kind: FileKind, status: &str) -> Result<ChangeKind> {
        if new_kind == FileKind::Conflict {
            return Ok(ChangeKind::Conflict);
        }
        if old_kind != FileKind::Conflict && Self::entry_type_changed(old_kind, new_kind) {
            return Ok(ChangeKind::TypeChanged);
        }
        match status {
            "added" | "copied" => Ok(ChangeKind::Added),
            "modified" => Ok(ChangeKind::Modified),
            "removed" => Ok(ChangeKind::Deleted),
            "renamed" => Ok(ChangeKind::Renamed),
            _ => Err(Error::Protocol {
                operation: "read jj changed files".to_owned(),
                detail: "jj returned an unknown file status",
            }),
        }
    }

    fn entry_type_changed(old_kind: FileKind, new_kind: FileKind) -> bool {
        old_kind != FileKind::Absent && new_kind != FileKind::Absent && old_kind != new_kind
    }

    fn add_stats(files: &mut [Self], output: &[u8]) -> Result<()> {
        if output.is_empty() {
            return Ok(());
        }
        let fields: Vec<_> = output.split(|byte| *byte == 0).collect();
        if fields.last() != Some(&&[][..]) || (fields.len() - 1) % 3 != 0 {
            return Err(Error::Protocol {
                operation: "read jj diff statistics".to_owned(),
                detail: "jj returned an invalid diff-stat record",
            });
        }
        let mut stats = HashMap::with_capacity((fields.len() - 1) / 3);
        for fields in fields[..fields.len() - 1].chunks_exact(3) {
            let parse = |value: &[u8]| {
                std::str::from_utf8(value)
                    .ok()
                    .and_then(|value| value.parse::<u64>().ok())
                    .ok_or_else(|| Error::Protocol {
                        operation: "read jj diff statistics".to_owned(),
                        detail: "jj returned an invalid line count",
                    })
            };
            stats.insert(fields[0], (parse(fields[1])?, parse(fields[2])?));
        }
        for file in files {
            if let Some(&(added, removed)) = stats.get(file.review_path().as_bytes()) {
                file.statistics = DiffStatistics {
                    lines_added: added,
                    lines_removed: removed,
                };
            }
        }
        Ok(())
    }

    fn parse_git(output: &[u8]) -> Result<Vec<Self>> {
        if output.is_empty() {
            return Ok(Vec::new());
        }
        let fields: Vec<_> = output.split(|byte| *byte == 0).collect();
        if fields.last() != Some(&&[][..]) {
            return Err(Error::Protocol {
                operation: "read Git changed files".to_owned(),
                detail: "Git returned an invalid raw diff",
            });
        }

        let mut files = Vec::new();
        let mut fields = fields[..fields.len() - 1].iter().copied();
        while let Some(metadata) = fields.next() {
            files.push(Self::parse_git_record(metadata, &mut fields)?);
        }
        files.sort_by(|left, right| {
            left.review_path()
                .as_bytes()
                .cmp(right.review_path().as_bytes())
        });
        Ok(files)
    }

    fn parse_git_record<'a>(
        metadata: &'a [u8],
        fields: &mut impl Iterator<Item = &'a [u8]>,
    ) -> Result<Self> {
        let metadata = GitChangeMetadata::parse(metadata)?;
        let first_path = fields.next().ok_or_else(|| Error::Protocol {
            operation: "read Git changed files".to_owned(),
            detail: "Git returned a changed file without a path",
        })?;
        let second_path = metadata.second_path(fields)?;
        let old_path =
            (metadata.old_kind != FileKind::Absent).then(|| RepoPath::from_bytes(first_path));
        let new_path = (metadata.new_kind != FileKind::Absent)
            .then(|| RepoPath::from_bytes(second_path.unwrap_or(first_path)));
        let display_path = Self::display_path(old_path.as_ref(), new_path.as_ref())
            .expect("Git raw changes always have a path");
        Ok(Self {
            old_path,
            new_path,
            old_kind: metadata.old_kind,
            new_kind: metadata.new_kind,
            change: metadata.change()?,
            display_path,
            statistics: DiffStatistics::default(),
        })
    }

    fn parse_git_stats(output: &[u8]) -> Result<BTreeMap<RepoPath, DiffStatistics>> {
        let fields: Vec<_> = output.split(|byte| *byte == 0).collect();
        if fields.last() != Some(&&[][..]) {
            return Err(Error::Protocol {
                operation: "read Git diff statistics".to_owned(),
                detail: "Git returned invalid diff statistics",
            });
        }
        let mut statistics = BTreeMap::new();
        let mut fields = fields[..fields.len() - 1].iter().copied();
        while let Some(record) = fields.next() {
            let mut values = record.splitn(3, |byte| *byte == b'\t');
            let added = values.next();
            let removed = values.next();
            let path = values.next();
            let (Some(added), Some(removed), Some(path)) = (added, removed, path) else {
                return Err(Error::Protocol {
                    operation: "read Git diff statistics".to_owned(),
                    detail: "Git returned incomplete diff statistics",
                });
            };
            let path = if path.is_empty() {
                let _old_path = fields.next();
                fields.next().ok_or_else(|| Error::Protocol {
                    operation: "read Git diff statistics".to_owned(),
                    detail: "Git returned an incomplete rename statistic",
                })?
            } else {
                path
            };
            let parse = |value: &[u8]| {
                if value == b"-" {
                    return Ok(0);
                }
                std::str::from_utf8(value)
                    .ok()
                    .and_then(|value| value.parse::<u64>().ok())
                    .ok_or_else(|| Error::Protocol {
                        operation: "read Git diff statistics".to_owned(),
                        detail: "Git returned an invalid line count",
                    })
            };
            statistics.insert(
                RepoPath::from_bytes(path),
                DiffStatistics {
                    lines_added: parse(added)?,
                    lines_removed: parse(removed)?,
                },
            );
        }
        Ok(statistics)
    }

    fn add_git_stats(files: &mut [Self], output: &[u8]) -> Result<()> {
        let statistics = Self::parse_git_stats(output)?;
        for file in files {
            if let Some(statistics) = statistics.get(file.review_path()) {
                file.statistics = *statistics;
            }
        }
        Ok(())
    }

    fn diff_paths(&self) -> impl Iterator<Item = &RepoPath> {
        self.old_path.iter().chain(
            self.new_path
                .iter()
                .filter(|new_path| self.old_path.as_ref() != Some(*new_path)),
        )
    }
}

/// The exact identity of one repository snapshot.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum SnapshotIdentity {
    /// A jj working-copy snapshot.
    Jj {
        /// The stable jj change ID.
        change_id: ChangeId,
        /// The exact jj commit ID.
        snapshot_id: SnapshotId,
        /// The full commit description.
        description: String,
        /// Repository-aware abbreviated ID, including jj terminal colours.
        display_id: String,
    },
    /// A Git working-tree snapshot.
    Git {
        /// The tree at `HEAD` that defines the review scope.
        base_tree: ReviewUnit,
        /// Abbreviated HEAD commit ID, or `unborn` before the first commit.
        display_id: String,
        /// The exact captured working-tree state.
        snapshot_id: SnapshotId,
    },
}

impl SnapshotIdentity {
    fn parse(output: &[u8]) -> Result<Self> {
        let raw_fields: Vec<_> = output.split(|byte| *byte == 0).collect();
        let fields: Vec<_> = raw_fields.iter().map(strip_ansi_escapes::strip).collect();
        if fields.len() != 5
            || fields[0].is_empty()
            || fields[1].is_empty()
            || fields[3].is_empty()
            || !fields[4].is_empty()
        {
            return Err(Error::Protocol {
                operation: "read jj snapshot identity".to_owned(),
                detail: "jj returned an invalid identity record",
            });
        }
        let change_id = std::str::from_utf8(&fields[0]).map_err(|_| Error::Protocol {
            operation: "read jj snapshot identity".to_owned(),
            detail: "jj returned a non-UTF-8 change ID",
        })?;
        let commit_id = std::str::from_utf8(&fields[1]).map_err(|_| Error::Protocol {
            operation: "read jj snapshot identity".to_owned(),
            detail: "jj returned a non-UTF-8 commit ID",
        })?;
        let description = std::str::from_utf8(&fields[2]).map_err(|_| Error::Protocol {
            operation: "read jj snapshot identity".to_owned(),
            detail: "jj returned a non-UTF-8 commit description",
        })?;

        Ok(Self::Jj {
            change_id: ChangeId(change_id.into()),
            snapshot_id: SnapshotId(commit_id.to_owned()),
            description: description.to_owned(),
            display_id: String::from_utf8(raw_fields[3].to_vec()).map_err(|_| Error::Protocol {
                operation: "read jj snapshot identity".to_owned(),
                detail: "jj returned a non-UTF-8 display ID",
            })?,
        })
    }

    /// Get the stable identifier used to group review marks.
    pub fn review_unit(&self) -> &ReviewUnit {
        match self {
            Self::Jj { change_id, .. } => &change_id.0,
            Self::Git { base_tree, .. } => base_tree,
        }
    }

    /// Get the exact identifier for the captured repository state.
    pub fn snapshot_id(&self) -> &str {
        match self {
            Self::Jj { snapshot_id, .. } | Self::Git { snapshot_id, .. } => snapshot_id.as_str(),
        }
    }

    /// Get the abbreviated revision identifier, with terminal colours when available.
    pub fn display_id(&self) -> &str {
        match self {
            Self::Jj { display_id, .. } | Self::Git { display_id, .. } => display_id,
        }
    }

    /// Get the text shown in the review header.
    pub fn description(&self) -> &str {
        match self {
            Self::Jj { description, .. } => description,
            Self::Git { .. } => "Git working tree\n",
        }
    }
}

/// A complete file-list snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Snapshot {
    /// The exact identity used for all snapshot commands.
    pub identity: SnapshotIdentity,
    /// Changed files sorted by escaped display path.
    pub files: Vec<ChangedFile>,
}

/// The result of one atomic poll attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PollResult {
    /// All data came from one exact commit.
    Complete(Snapshot),
    /// The working-copy commit changed before the poll completed.
    ChangedDuringPoll,
}

/// The difference from a stored review baseline.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Interdiff {
    /// The stored commit no longer exists.
    MissingBaseline,
    /// The Git-style difference from the stored commit.
    Diff(Vec<u8>),
}

trait RepositoryBackend: std::fmt::Debug + Send + Sync {
    fn set_state_root(&self, repository_root: &Path, state_root: &Path);
    fn current_identity(&self, repository: &Repository) -> Result<SnapshotIdentity>;
    fn read_files(
        &self,
        repository: &Repository,
        identity: &SnapshotIdentity,
    ) -> Result<Vec<ChangedFile>>;
    fn read_stats(
        &self,
        repository: &Repository,
        identity: &SnapshotIdentity,
        files: &mut [ChangedFile],
    ) -> Result<()>;
    fn diff(
        &self,
        repository: &Repository,
        snapshot: &Snapshot,
        file: &ChangedFile,
    ) -> Result<Vec<u8>>;
    fn file_at(&self, repository: &Repository, revision: &str, path: &RepoPath) -> Result<Vec<u8>>;
    fn base_file_at(
        &self,
        repository: &Repository,
        snapshot: &Snapshot,
        path: &RepoPath,
    ) -> Result<Vec<u8>>;
    fn interdiff(
        &self,
        repository: &Repository,
        baseline_snapshot_id: &str,
        snapshot: &Snapshot,
        path: &RepoPath,
    ) -> Result<Interdiff>;
    fn compare_baselines(
        &self,
        repository: &Repository,
        snapshot: &Snapshot,
        plan: &BaselineComparisonPlan,
    ) -> Result<BaselineComparisonResults>;
}

/// The repository implementation selected during discovery.
#[derive(Clone, Copy, Debug, Eq, PartialEq, strum::Display, strum::EnumString)]
#[strum(serialize_all = "lowercase")]
pub enum RepoType {
    /// A Git working tree.
    Git,
    /// A Jujutsu workspace.
    Jj,
}

/// A direction from the current jj working-copy commit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RevisionDirection {
    /// Direct parent commits.
    Parents,
    /// Direct child commits.
    Children,
}

/// One mutable jj commit that the reviewer can open.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RevisionCandidate {
    /// The full stable jj change identifier used by `jj edit`.
    pub change_id: ChangeId,
    /// The short change identifier shown in the selector.
    pub short_change_id: String,
    /// The first line of the commit description.
    pub description: String,
}

/// One terminal-rendered row in the revision history.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RevisionHistoryLine {
    /// ANSI-colored text produced by `jj log`.
    pub text: String,
    /// Text without terminal control sequences, used if ANSI parsing fails.
    pub plain_text: String,
    /// The short change identifier shown in the terminal-rendered text.
    pub short_change_id: Option<String>,
    /// The full change identifier when this row represents a commit.
    pub change_id: Option<ChangeId>,
    /// Whether this row represents the current working-copy commit.
    pub is_current: bool,
    /// Whether this row is immutable context that cannot be selected.
    pub is_immutable: bool,
}

/// A discovered jj or Git workspace.
#[derive(Clone, Debug)]
pub struct Repository {
    root: PathBuf,
    repo_type: RepoType,
    backend: Arc<dyn RepositoryBackend>,
    cancellation: Cancellation,
}

impl Repository {
    /// Find the canonical jj or Git workspace root for a directory.
    pub fn discover(start: impl AsRef<Path>) -> Result<Self> {
        let start = start.as_ref();
        let cancellation = Cancellation::default();
        let jj_root = if let Ok(output) =
            RepositoryProcess::new("jj", start, "discover jj repository", &cancellation)
                .output(["root"])
            && output.status.success()
        {
            let root = trim_line_ending(&output.stdout);
            if root.is_empty() {
                return Err(Error::Protocol {
                    operation: "discover jj repository".to_owned(),
                    detail: "jj root returned an empty path",
                });
            }
            Some(PathBuf::from(OsString::from_vec(root.to_vec())))
        } else {
            None
        };

        let output = RepositoryProcess::new("git", start, "discover Git repository", &cancellation)
            .output(["rev-parse", "--show-toplevel"])?;
        let git_root = if output.status.success() {
            let root = trim_line_ending(&output.stdout);
            if root.is_empty() {
                return Err(Error::Protocol {
                    operation: "discover Git repository".to_owned(),
                    detail: "git rev-parse returned an empty path",
                });
            }
            Some(PathBuf::from(OsString::from_vec(root.to_vec())))
        } else {
            None
        };

        if let Some(root) = jj_root.as_ref()
            && git_root
                .as_ref()
                .is_none_or(|git_root| root.components().count() >= git_root.components().count())
        {
            return Ok(Self {
                root: root.clone(),
                repo_type: RepoType::Jj,
                backend: Arc::new(jj::JjBackend::default()),
                cancellation,
            });
        }

        let Some(root) = git_root else {
            return Err(Error::NotRepository {
                path: start.to_owned(),
            });
        };
        let objects =
            RepositoryProcess::new("git", &root, "discover Git object directory", &cancellation)
                .output([
                    "rev-parse",
                    "--path-format=absolute",
                    "--git-path",
                    "objects",
                ])?;
        if !objects.status.success() {
            return Err(Error::CommandFailed {
                operation: "discover Git object directory".to_owned(),
                code: objects.status.code(),
            });
        }
        let repository_objects = trim_line_ending(&objects.stdout);
        if repository_objects.is_empty() {
            return Err(Error::Protocol {
                operation: "discover Git object directory".to_owned(),
                detail: "git rev-parse returned an empty object path",
            });
        }

        Ok(Self {
            root,
            repo_type: RepoType::Git,
            backend: Arc::new(git::GitBackend::new(PathBuf::from(OsString::from_vec(
                repository_objects.to_vec(),
            )))),
            cancellation,
        })
    }

    /// Set the plugin state directory used for Git snapshot objects.
    #[must_use]
    pub fn with_state_root(self, state_root: impl AsRef<Path>) -> Self {
        self.backend.set_state_root(&self.root, state_root.as_ref());
        self
    }

    /// Get the canonical workspace root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Get the repository type selected during discovery.
    pub fn repo_type(&self) -> RepoType {
        self.repo_type
    }

    /// List visible mutable commits next to the current jj commit.
    pub fn revision_candidates(
        &self,
        direction: RevisionDirection,
    ) -> Result<Vec<RevisionCandidate>> {
        if self.repo_type != RepoType::Jj {
            return Ok(Vec::new());
        }
        jj::revision_candidates(self, direction)
    }

    /// Render the mutable graph above the immutable base and its immutable children with `jj log`.
    pub fn revision_history(&self) -> Result<Vec<RevisionHistoryLine>> {
        if self.repo_type != RepoType::Jj {
            return Ok(Vec::new());
        }
        jj::revision_history(self)
    }

    /// Make one mutable jj change the working-copy commit.
    pub fn edit_revision(&self, change_id: &ChangeId) -> Result<bool> {
        if self.repo_type != RepoType::Jj {
            return Ok(false);
        }
        let revset = format!("{} & mutable()", change_id.as_str());
        let editable = self
            .run_jj([
                "--ignore-working-copy",
                "log",
                "--no-graph",
                "-r",
                &revset,
                "-T",
                r#"change_id ++ "\n""#,
            ])?
            .stdout;
        if editable.is_empty() {
            return Ok(false);
        }
        self.run_jj(["edit", change_id.as_str()]).map(|_| true)
    }

    /// Cancel the active repository command during shutdown.
    pub fn cancel(&self) {
        self.cancellation.cancel();
    }

    /// Build one complete snapshot or reject a mixed poll.
    pub fn poll(&self) -> Result<PollResult> {
        let identity = self.current_identity()?;
        let mut files = self.backend.read_files(self, &identity)?;
        self.backend.read_stats(self, &identity, &mut files)?;
        let verified = self.current_identity()?;

        if identity != verified {
            return Ok(PollResult::ChangedDuringPoll);
        }

        Ok(PollResult::Complete(Snapshot { identity, files }))
    }

    /// Snapshot and read the current change identity.
    pub fn current_identity(&self) -> Result<SnapshotIdentity> {
        self.backend.current_identity(self)
    }

    /// Read and parse the full Git-style diff for one changed file.
    pub fn diff(&self, snapshot: &Snapshot, file: &ChangedFile) -> Result<Vec<u8>> {
        self.backend.diff(self, snapshot, file)
    }

    /// Read one file at an exact revision.
    pub fn file_at(&self, revision: &str, path: &RepoPath) -> Result<Vec<u8>> {
        self.backend.file_at(self, revision, path)
    }

    /// Read one file from the base of an exact snapshot.
    pub fn base_file_at(&self, snapshot: &Snapshot, path: &RepoPath) -> Result<Vec<u8>> {
        self.backend.base_file_at(self, snapshot, path)
    }

    /// Compare one path between a stored baseline and an exact snapshot.
    pub fn interdiff(
        &self,
        baseline_snapshot_id: &str,
        snapshot: &Snapshot,
        path: &RepoPath,
    ) -> Result<Interdiff> {
        self.backend
            .interdiff(self, baseline_snapshot_id, snapshot, path)
    }

    /// Execute all stored-baseline comparisons in one backend plan.
    pub fn compare_baselines(
        &self,
        snapshot: &Snapshot,
        plan: &BaselineComparisonPlan,
    ) -> Result<BaselineComparisonResults> {
        self.backend.compare_baselines(self, snapshot, plan)
    }

    fn read_jj_identity(&self, ignore_working_copy: bool) -> Result<SnapshotIdentity> {
        let mut arguments = Vec::new();
        if ignore_working_copy {
            arguments.push(OsString::from("--ignore-working-copy"));
        }
        arguments.extend([
            OsString::from("--color=always"),
            OsString::from("log"),
            OsString::from("--no-graph"),
            OsString::from("-r"),
            OsString::from("@"),
            OsString::from("-T"),
            OsString::from(jj::IDENTITY_TEMPLATE),
        ]);
        SnapshotIdentity::parse(&self.run_jj(arguments)?.stdout)
    }

    fn run_jj<I, S>(&self, arguments: I) -> Result<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = self.output_jj(arguments)?;
        if !output.status.success() {
            return Err(Error::CommandFailed {
                operation: "read jj repository".to_owned(),
                code: output.status.code(),
            });
        }
        Ok(output)
    }

    fn output_jj<I, S>(&self, arguments: I) -> Result<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        RepositoryProcess::new("jj", &self.root, "read jj repository", &self.cancellation)
            .output(arguments)
    }
}

impl<'a> RepositoryProcess<'a> {
    fn new(
        program: &'static str,
        cwd: &'a Path,
        operation: &'static str,
        cancellation: &'a Cancellation,
    ) -> Self {
        Self {
            program,
            cwd,
            operation,
            cancellation,
            environment: &[],
        }
    }

    fn with_environment(mut self, environment: &'a [(OsString, OsString)]) -> Self {
        self.environment = environment;
        self
    }

    fn output<I, S>(self, arguments: I) -> Result<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut command = Command::new(self.program);
        if self.program == "jj" {
            command.args(["--color=never", "--no-pager"]);
        } else {
            command.args(["--no-pager", "-c", "color.ui=false"]);
        }
        let mut child = command
            .args(arguments)
            .envs(self.environment.iter().cloned())
            .current_dir(self.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| Error::Spawn {
                operation: self.operation.to_owned(),
                program: OsString::from(self.program),
                current_dir: Some(self.cwd.to_owned()),
                source,
            })?;
        let bytes = Arc::new(AtomicUsize::new(0));
        let exceeded = Arc::new(AtomicBool::new(false));
        let stdout = Self::capture(
            child.stdout.take().expect("stdout is piped"),
            Arc::clone(&bytes),
            Arc::clone(&exceeded),
        );
        let stderr = Self::capture(
            child.stderr.take().expect("stderr is piped"),
            bytes,
            Arc::clone(&exceeded),
        );
        let mut cancelled = false;
        let status = loop {
            if exceeded.load(Ordering::Relaxed) {
                let _ = child.kill();
                break child.wait();
            }
            if self.cancellation.is_cancelled() {
                cancelled = true;
                let _ = child.kill();
                break child.wait();
            }
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) => thread::sleep(Duration::from_millis(10)),
                Err(error) => break Err(error),
            }
        }
        .map_err(|source| Error::Spawn {
            operation: self.operation.to_owned(),
            program: OsString::from("jj"),
            current_dir: Some(self.cwd.to_owned()),
            source,
        })?;
        let stdout = stdout
            .join()
            .expect("output reader did not panic")
            .map_err(|source| Error::Spawn {
                operation: self.operation.to_owned(),
                program: OsString::from(self.program),
                current_dir: Some(self.cwd.to_owned()),
                source,
            })?;
        let stderr = stderr
            .join()
            .expect("output reader did not panic")
            .map_err(|source| Error::Spawn {
                operation: self.operation.to_owned(),
                program: OsString::from(self.program),
                current_dir: Some(self.cwd.to_owned()),
                source,
            })?;
        if cancelled {
            return Err(Error::CommandCancelled {
                operation: self.operation.to_owned(),
                path: self.cwd.to_owned(),
            });
        }
        if exceeded.load(Ordering::Relaxed) {
            return Err(Error::CommandOutputTooLarge {
                operation: self.operation.to_owned(),
                path: self.cwd.to_owned(),
            });
        }
        Ok(Output {
            status,
            stdout,
            stderr,
        })
    }

    fn capture(
        mut pipe: impl Read + Send + 'static,
        bytes: Arc<AtomicUsize>,
        exceeded: Arc<AtomicBool>,
    ) -> thread::JoinHandle<std::io::Result<Vec<u8>>> {
        thread::spawn(move || {
            let mut output = Vec::new();
            let mut buffer = [0; 8192];
            loop {
                let count = pipe.read(&mut buffer)?;
                if count == 0 {
                    return Ok(output);
                }
                let previous = bytes.fetch_add(count, Ordering::Relaxed);
                if previous.saturating_add(count) > COMMAND_OUTPUT_LIMIT {
                    exceeded.store(true, Ordering::Relaxed);
                    return Ok(output);
                }
                output.extend_from_slice(&buffer[..count]);
            }
        })
    }
}

fn trim_line_ending(bytes: &[u8]) -> &[u8] {
    bytes
        .strip_suffix(b"\r\n")
        .or_else(|| bytes.strip_suffix(b"\n"))
        .unwrap_or(bytes)
}

#[cfg(test)]
#[path = "repository.tests.rs"]
mod tests;
