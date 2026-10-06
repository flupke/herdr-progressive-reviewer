//! Stable snapshots of one jj change or Git working tree.

use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;

#[cfg(target_os = "linux")]
use nix::{
    errno::Errno,
    sys::wait::{Id, WaitPidFlag, waitid},
    unistd::Pid,
};

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
    /// The options that keep the program's output plain, before the command's arguments.
    options: &'static [&'static str],
    /// The most output the command may print, stdout and stderr together.
    output_limit: usize,
    cwd: &'a Path,
    operation: &'static str,
    cancellation: &'a Cancellation,
    environment: &'a [(OsString, OsString)],
}

/// Cancels a repository's commands, and wakes the commands that wait on a child.
///
/// A command waits on the condition variable until its child exits, its output passes the
/// size limit, or the repository is cancelled; its threads wake it through
/// [`Cancellation::wake`].
#[derive(Clone, Debug, Default)]
struct Cancellation(Arc<CancellationSignal>);

#[derive(Debug, Default)]
struct CancellationSignal {
    cancelled: Mutex<bool>,
    changed: Condvar,
}

impl Cancellation {
    fn cancel(&self) {
        *self.lock() = true;
        self.0.changed.notify_all();
    }

    fn is_cancelled(&self) -> bool {
        *self.lock()
    }

    /// Wake the commands that wait, so each checks whether it is over.
    fn wake(&self) {
        let _cancelled = self.lock();
        self.0.changed.notify_all();
    }

    /// Wait until `settled` holds or the repository is cancelled, and say whether it was.
    #[cfg(target_os = "linux")]
    fn wait_until(&self, settled: impl Fn() -> bool) -> bool {
        let mut cancelled = self.lock();
        while !*cancelled && !settled() {
            cancelled = self
                .0
                .changed
                .wait(cancelled)
                .unwrap_or_else(PoisonError::into_inner);
        }
        *cancelled
    }

    fn lock(&self) -> MutexGuard<'_, bool> {
        self.0
            .cancelled
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

/// What the threads of one command tell the command that waits on them: how much output its
/// readers took, and whether its child exited.
#[derive(Debug, Default)]
struct CommandProgress {
    bytes: AtomicUsize,
    exceeded: AtomicBool,
    #[cfg(target_os = "linux")]
    exited: AtomicBool,
}

impl CommandProgress {
    fn exceeded(&self) -> bool {
        self.exceeded.load(Ordering::SeqCst)
    }

    /// Whether the command is over: its child exited, or its output passed the size limit.
    #[cfg(target_os = "linux")]
    fn settled(&self) -> bool {
        self.exceeded() || self.exited.load(Ordering::SeqCst)
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

/// An abbreviated revision identifier (a snapshot's `display_id`) as jj highlights it: the
/// shortest prefix that names the revision, then the rest of the abbreviation. An identifier
/// with no prefix coloured apart, such as a Git abbreviation, is all rest.
#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Serialize, ts_rs::TS)]
pub struct ShortRevision {
    /// The shortest prefix that names the revision; empty when none is coloured apart.
    pub prefix: String,
    /// The rest of the abbreviation, after the prefix.
    pub rest: String,
}

impl ShortRevision {
    /// The revision `display_id` names, from the colours jj gives its two parts: the first
    /// run of text between two colours is the prefix, the runs after it are the rest. A
    /// single run is all rest.
    fn of(display_id: &str) -> Self {
        let mut runs = Vec::new();
        let mut run = String::new();
        let mut characters = display_id.chars();
        while let Some(character) = characters.next() {
            if character == '\u{1b}' {
                // A control sequence ends with its final byte, from `@` to `~`.
                if characters.next() == Some('[') {
                    characters.find(|character| ('@'..='~').contains(character));
                }
                if !run.is_empty() {
                    runs.push(std::mem::take(&mut run));
                }
            } else {
                run.push(character);
            }
        }
        if !run.is_empty() {
            runs.push(run);
        }
        if runs.len() < 2 {
            return Self {
                prefix: String::new(),
                rest: runs.concat(),
            };
        }
        let mut runs = runs.into_iter();
        Self {
            prefix: runs.next().unwrap_or_default(),
            rest: runs.collect(),
        }
    }
}

/// Added and removed text lines in one diff.
#[derive(
    Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS,
)]
pub struct DiffStatistics {
    /// Number of added text lines.
    pub lines_added: u64,
    /// Number of removed text lines.
    pub lines_removed: u64,
}

impl DiffStatistics {
    /// The added and removed lines.
    pub fn lines(&self) -> u64 {
        self.lines_added + self.lines_removed
    }

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

    /// Whether the change adds or removes text lines. A binary file, a mode change, a pure
    /// rename and an empty file change none: they are reviewed whole.
    pub fn changes_text_lines(&self) -> bool {
        self.statistics.lines() > 0
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

    /// Get the abbreviated revision identifier, split where jj highlights it.
    pub fn short_revision(&self) -> ShortRevision {
        ShortRevision::of(self.display_id())
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
            options: if program == "jj" {
                &["--color=never", "--no-pager"]
            } else {
                &["--no-pager", "-c", "color.ui=false"]
            },
            output_limit: COMMAND_OUTPUT_LIMIT,
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
        let mut child = Command::new(self.program)
            .args(self.options)
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
        let progress = Arc::<CommandProgress>::default();
        let stdout = self.capture(
            child.stdout.take().expect("stdout is piped"),
            Arc::clone(&progress),
        );
        let stderr = self.capture(
            child.stderr.take().expect("stderr is piped"),
            Arc::clone(&progress),
        );
        let (status, cancelled) = self.wait(&mut child, &progress);
        let status = status.map_err(|source| Error::Spawn {
            operation: self.operation.to_owned(),
            program: OsString::from(self.program),
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
        if progress.exceeded() {
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

    /// Wait until the child exits, its output passes the size limit, or the repository is
    /// cancelled; kill the child in the last two cases. Return its status, and whether the
    /// cancellation stopped it: the size limit wins, as it reports the command's own failure.
    #[cfg(target_os = "linux")]
    fn wait(
        &self,
        child: &mut Child,
        progress: &Arc<CommandProgress>,
    ) -> (std::io::Result<ExitStatus>, bool) {
        let exit = Self::watch_exit(child, Arc::clone(progress), self.cancellation.clone());
        let cancelled = self.cancellation.wait_until(|| progress.settled()) && !progress.exceeded();
        if cancelled || progress.exceeded() {
            let _ = child.kill();
        }
        // The watcher returns once the child exits, and never reaps it: its process ID stays
        // the child's until `wait` below.
        exit.join().expect("exit watcher did not panic");
        (child.wait(), cancelled)
    }

    /// Wait until the child exits, its output passes the size limit, or the repository is
    /// cancelled; kill the child in the last two cases. Return its status, and whether the
    /// cancellation stopped it: the size limit wins, as it reports the command's own failure.
    ///
    /// Without `waitid`, nothing wakes the command when its child exits, so it checks every
    /// 10 ms.
    #[cfg(not(target_os = "linux"))]
    fn wait(
        &self,
        child: &mut Child,
        progress: &Arc<CommandProgress>,
    ) -> (std::io::Result<ExitStatus>, bool) {
        loop {
            if progress.exceeded() {
                let _ = child.kill();
                return (child.wait(), false);
            }
            if self.cancellation.is_cancelled() {
                let _ = child.kill();
                return (child.wait(), true);
            }
            match child.try_wait() {
                Ok(Some(status)) => return (Ok(status), false),
                Ok(None) => thread::sleep(std::time::Duration::from_millis(10)),
                Err(error) => return (Err(error), false),
            }
        }
    }

    /// Read one pipe on its own thread, and wake the command once the output passes the size
    /// limit.
    fn capture(
        &self,
        mut pipe: impl Read + Send + 'static,
        progress: Arc<CommandProgress>,
    ) -> thread::JoinHandle<std::io::Result<Vec<u8>>> {
        let cancellation = self.cancellation.clone();
        let limit = self.output_limit;
        thread::spawn(move || {
            let mut output = Vec::new();
            let mut buffer = [0; 8192];
            loop {
                let count = pipe.read(&mut buffer)?;
                if count == 0 {
                    return Ok(output);
                }
                let previous = progress.bytes.fetch_add(count, Ordering::Relaxed);
                if previous.saturating_add(count) > limit {
                    progress.exceeded.store(true, Ordering::SeqCst);
                    cancellation.wake();
                    return Ok(output);
                }
                output.extend_from_slice(&buffer[..count]);
            }
        })
    }

    /// Wake the command once its child exits, on a thread of its own. The thread leaves the
    /// child unreaped, so the command can still kill it by its process ID and must reap it.
    #[cfg(target_os = "linux")]
    fn watch_exit(
        child: &Child,
        progress: Arc<CommandProgress>,
        cancellation: Cancellation,
    ) -> thread::JoinHandle<()> {
        let pid = Pid::from_raw(i32::try_from(child.id()).expect("a process ID fits an i32"));
        thread::spawn(move || {
            // Any error other than an interruption means the child cannot be waited for any
            // more; the command's own `wait` then reports it.
            while let Err(Errno::EINTR) =
                waitid(Id::Pid(pid), WaitPidFlag::WEXITED | WaitPidFlag::WNOWAIT)
            {}
            progress.exited.store(true, Ordering::SeqCst);
            cancellation.wake();
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
