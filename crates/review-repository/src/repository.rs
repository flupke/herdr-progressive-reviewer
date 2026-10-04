//! Stable snapshots of one jj change or Git working tree.

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
mod jj;
mod watch;

pub use comparison_plan::{BaselineComparison, BaselineComparisonPlan, BaselineComparisonResults};
pub use watch::{MetadataScope, MetadataWatch, WatchPlan};

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

    fn sort_by_review_path(files: &mut [Self]) {
        files.sort_by(|left, right| {
            left.review_path()
                .as_bytes()
                .cmp(right.review_path().as_bytes())
        });
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

    fn entry_type_changed(old_kind: FileKind, new_kind: FileKind) -> bool {
        old_kind != FileKind::Absent && new_kind != FileKind::Absent && old_kind != new_kind
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

    /// Get the abbreviated revision identifier, without terminal colours.
    pub fn plain_display_id(&self) -> String {
        String::from_utf8_lossy(&strip_ansi_escapes::strip(self.display_id())).into_owned()
    }

    /// The first line of the description; empty when it has none.
    pub fn title(&self) -> &str {
        self.description().lines().next().unwrap_or_default()
    }

    /// What the change says it does: a jj change's description, when it has
    /// one. A Git working tree has none.
    pub fn change_description(&self) -> Option<&str> {
        match self {
            Self::Jj { description, .. } => {
                Some(description.trim()).filter(|description| !description.is_empty())
            }
            Self::Git { .. } => None,
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
    fn repo_type(&self) -> RepoType;
    fn set_state_root(&self, repository_root: &Path, state_root: &Path);
    fn watch_plan(&self, root: &Path) -> WatchPlan;
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
    fn revision_candidates(
        &self,
        repository: &Repository,
        direction: RevisionDirection,
    ) -> Result<Vec<RevisionCandidate>>;
    fn revision_history(&self, repository: &Repository) -> Result<Vec<RevisionHistoryLine>>;
    fn edit_revision(&self, repository: &Repository, change_id: &ChangeId) -> Result<bool>;
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
    backend: Arc<dyn RepositoryBackend>,
    cancellation: Cancellation,
}

impl Repository {
    /// Find the canonical jj or Git workspace root for a directory.
    ///
    /// The innermost workspace wins; jj wins when both share a root.
    pub fn discover(start: impl AsRef<Path>) -> Result<Self> {
        let start = start.as_ref();
        let cancellation = Cancellation::default();
        let jj_root = jj::JjBackend::workspace_root(start, &cancellation)?;
        let git_root = git::GitBackend::working_tree_root(start, &cancellation)?;

        if let Some(root) = jj_root
            && git_root
                .as_ref()
                .is_none_or(|git_root| root.components().count() >= git_root.components().count())
        {
            return Ok(Self {
                root,
                backend: Arc::new(jj::JjBackend::default()),
                cancellation,
            });
        }

        let Some(root) = git_root else {
            return Err(Error::NotRepository {
                path: start.to_owned(),
            });
        };
        let backend = git::GitBackend::discover(&root, &cancellation)?;
        Ok(Self {
            root,
            backend: Arc::new(backend),
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
        self.backend.repo_type()
    }

    /// Describe the filesystem locations whose changes can produce a new snapshot.
    pub fn watch_plan(&self) -> WatchPlan {
        self.backend.watch_plan(&self.root)
    }

    /// List visible mutable commits next to the current commit, when the backend has revisions.
    pub fn revision_candidates(
        &self,
        direction: RevisionDirection,
    ) -> Result<Vec<RevisionCandidate>> {
        self.backend.revision_candidates(self, direction)
    }

    /// Render the mutable graph above the immutable base and its immutable children.
    ///
    /// Backends without revisions return no lines.
    pub fn revision_history(&self) -> Result<Vec<RevisionHistoryLine>> {
        self.backend.revision_history(self)
    }

    /// Make one mutable change the working-copy commit.
    ///
    /// Returns `false` when the change cannot be edited or the backend has no revisions.
    pub fn edit_revision(&self, change_id: &ChangeId) -> Result<bool> {
        self.backend.edit_revision(self, change_id)
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

/// Read the single path a discovery command printed, rejecting an empty one.
fn path_from_output(
    output: &[u8],
    operation: &'static str,
    empty_detail: &'static str,
) -> Result<PathBuf> {
    let path = trim_line_ending(output);
    if path.is_empty() {
        return Err(Error::Protocol {
            operation: operation.to_owned(),
            detail: empty_detail,
        });
    }
    Ok(PathBuf::from(OsString::from_vec(path.to_vec())))
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
