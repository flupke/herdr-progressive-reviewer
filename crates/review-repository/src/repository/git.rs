//! Git working-tree snapshots backed by a private index and object store.

use std::ffi::{OsStr, OsString};
use std::fmt::Write as _;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::{Arc, Mutex};

use ignore::gitignore::gitconfig_excludes_path;
use sha2::{Digest, Sha256};

use super::watch::{MetadataScope, resolve_directory_or_link_file};
use super::{
    BaselineComparison, BaselineComparisonPlan, BaselineComparisonResults, Cancellation, ChangeId,
    ChangedFile, Interdiff, RepoPath, RepoType, Repository, RepositoryBackend, RepositoryProcess,
    RevisionCandidate, RevisionDirection, RevisionHistoryLine, Snapshot, SnapshotIdentity,
    WatchPlan, path_from_output, trim_line_ending,
};
use crate::{Error, Result};

mod records;

#[derive(Debug)]
pub(super) struct GitBackend {
    repository_objects: PathBuf,
    cache: Mutex<Option<Arc<GitCache>>>,
}

#[derive(Debug)]
struct GitCache {
    index: PathBuf,
    objects: PathBuf,
    repository_objects: PathBuf,
    base_tree: Mutex<Option<String>>,
}

impl GitBackend {
    /// Find the Git working tree that contains `start`, if any.
    pub(super) fn working_tree_root(
        start: &Path,
        cancellation: &Cancellation,
    ) -> Result<Option<PathBuf>> {
        let output = RepositoryProcess::new("git", start, "discover Git repository", cancellation)
            .output(["rev-parse", "--show-toplevel"])?;
        if !output.status.success() {
            return Ok(None);
        }
        path_from_output(
            &output.stdout,
            "discover Git repository",
            "git rev-parse returned an empty path",
        )
        .map(Some)
    }

    /// Open the backend for the working tree at `root`.
    pub(super) fn discover(root: &Path, cancellation: &Cancellation) -> Result<Self> {
        let objects =
            RepositoryProcess::new("git", root, "discover Git object directory", cancellation)
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
        Ok(Self {
            repository_objects: path_from_output(
                &objects.stdout,
                "discover Git object directory",
                "git rev-parse returned an empty object path",
            )?,
            cache: Mutex::new(None),
        })
    }

    /// Watch HEAD, the index and refs of the plan's working tree.
    ///
    /// Returns the common Git directory, or `None` when the working tree has no Git directory.
    pub(super) fn watch_metadata(plan: &mut WatchPlan) -> Option<PathBuf> {
        let git = Self::git_directory(&plan.root)?;
        let common =
            resolve_directory_or_link_file(&git.join("commondir")).unwrap_or_else(|| git.clone());
        plan.add_metadata(&git, MetadataScope::Entries);
        plan.add_metadata(&common, MetadataScope::Entries);
        plan.add_metadata(&common.join("refs"), MetadataScope::Subtree);
        Some(common)
    }

    /// Resolve the Git directory of the working tree at `root`, following a `.git` link file.
    fn git_directory(root: &Path) -> Option<PathBuf> {
        let path = root.join(".git");
        if path.is_dir() {
            return std::fs::canonicalize(&path).ok();
        }
        let contents = std::fs::read_to_string(&path).ok()?;
        let value = contents.trim().strip_prefix("gitdir:")?.trim();
        let directory = path.parent()?.join(value);
        std::fs::canonicalize(directory).ok()
    }

    fn cache(&self) -> Result<Arc<GitCache>> {
        self.cache
            .lock()
            .map_err(|_| Error::Protocol {
                operation: "snapshot Git repository".to_owned(),
                detail: "Git snapshot state lock was poisoned",
            })?
            .clone()
            .ok_or_else(|| Error::Protocol {
                operation: "snapshot Git repository".to_owned(),
                detail: "Git snapshot storage is not configured",
            })
    }

    fn run<I, S>(&self, repository: &Repository, arguments: I) -> Result<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = self.output(repository, arguments)?;
        if !output.status.success() {
            return Err(Error::CommandFailed {
                operation: "read Git repository".to_owned(),
                code: output.status.code(),
            });
        }
        Ok(output)
    }

    fn output<I, S>(&self, repository: &Repository, arguments: I) -> Result<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let environment = self
            .cache
            .lock()
            .map_err(|_| Error::Protocol {
                operation: "read Git repository".to_owned(),
                detail: "Git snapshot state lock was poisoned",
            })?
            .as_deref()
            .map(git_environment)
            .unwrap_or_default();
        RepositoryProcess::new(
            "git",
            &repository.root,
            "read Git repository",
            &repository.cancellation,
        )
        .with_environment(&environment)
        .output(arguments)
    }

    fn run_with_cache<I, S>(
        repository: &Repository,
        cache: &GitCache,
        arguments: I,
    ) -> Result<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let environment = git_environment(cache);
        let output = RepositoryProcess::new(
            "git",
            &repository.root,
            "snapshot Git repository",
            &repository.cancellation,
        )
        .with_environment(&environment)
        .output(arguments)?;
        if !output.status.success() {
            return Err(Error::CommandFailed {
                operation: "snapshot Git repository".to_owned(),
                code: output.status.code(),
            });
        }
        Ok(output)
    }

    fn identity(&self, repository: &Repository, cache: &GitCache) -> Result<SnapshotIdentity> {
        std::fs::create_dir_all(&cache.objects).map_err(|source| Error::Io {
            operation: "create Git snapshot storage",
            path: cache.objects.clone(),
            source,
        })?;
        let base = self.output(repository, ["rev-parse", "--verify", "HEAD^{tree}"])?;
        let base_tree = if base.status.success() {
            parse_tree_id(&base.stdout, "Git returned a non-UTF-8 tree ID")?
        } else {
            parse_tree_id(
                &self.run(repository, ["mktree"])?.stdout,
                "Git returned a non-UTF-8 empty tree ID",
            )?
        };
        let head = self.output(repository, ["rev-parse", "--verify", "--short", "HEAD"])?;
        let display_id = if head.status.success() {
            parse_tree_id(&head.stdout, "Git returned a non-UTF-8 HEAD commit ID")?
        } else {
            "unborn".to_owned()
        };
        let mut cached_base = cache.base_tree.lock().map_err(|_| Error::Protocol {
            operation: "snapshot Git repository".to_owned(),
            detail: "Git snapshot state lock was poisoned",
        })?;
        if cached_base.as_deref() != Some(&base_tree) || !cache.index.exists() {
            Self::run_with_cache(repository, cache, ["read-tree", base_tree.as_str()])?;
            *cached_base = Some(base_tree.clone());
        }
        Self::run_with_cache(repository, cache, ["add", "-A", "--", "."])?;
        let snapshot_tree = parse_tree_id(
            &Self::run_with_cache(repository, cache, ["write-tree"])?.stdout,
            "Git returned a non-UTF-8 snapshot tree ID",
        )?;
        Ok(SnapshotIdentity::Git {
            base_tree: base_tree.into(),
            display_id,
            snapshot_id: snapshot_tree.into(),
        })
    }
}

impl RepositoryBackend for GitBackend {
    fn repo_type(&self) -> RepoType {
        RepoType::Git
    }

    /// Watch HEAD, the index and refs, and apply Git's global and repository excludes.
    fn watch_plan(&self, root: &Path) -> WatchPlan {
        let mut plan = WatchPlan::new(root);
        let mut excludes = gitconfig_excludes_path().into_iter().collect::<Vec<_>>();
        if let Some(common) = Self::watch_metadata(&mut plan) {
            excludes.push(common.join("info/exclude"));
        }
        plan.git_excludes = Some(excludes);
        plan
    }

    fn set_state_root(&self, repository_root: &Path, state_root: &Path) {
        let digest = Sha256::digest(repository_root.as_os_str().as_bytes());
        let repository_key = digest.iter().fold(String::new(), |mut key, byte| {
            write!(key, "{byte:02x}").expect("writing to a string cannot fail");
            key
        });
        let directory = state_root.join("git").join(repository_key);
        *self.cache.lock().expect("new Git snapshot lock is valid") = Some(Arc::new(GitCache {
            index: directory.join(format!("working-index-{}", std::process::id())),
            objects: directory.join("objects"),
            repository_objects: self.repository_objects.clone(),
            base_tree: Mutex::new(None),
        }));
    }

    fn current_identity(&self, repository: &Repository) -> Result<SnapshotIdentity> {
        let cache = self.cache()?;
        self.identity(repository, &cache)
    }

    fn read_files(
        &self,
        repository: &Repository,
        identity: &SnapshotIdentity,
    ) -> Result<Vec<ChangedFile>> {
        ChangedFile::parse_git(
            &self
                .run(
                    repository,
                    [
                        "diff",
                        "--raw",
                        "-z",
                        "--find-renames",
                        identity.review_unit().as_str(),
                        identity.snapshot_id(),
                    ],
                )?
                .stdout,
        )
    }

    fn read_stats(
        &self,
        repository: &Repository,
        identity: &SnapshotIdentity,
        files: &mut [ChangedFile],
    ) -> Result<()> {
        ChangedFile::add_git_stats(
            files,
            &self
                .run(
                    repository,
                    [
                        "diff",
                        "--numstat",
                        "-z",
                        "--find-renames",
                        identity.review_unit().as_str(),
                        identity.snapshot_id(),
                    ],
                )?
                .stdout,
        )
    }

    fn diff(
        &self,
        repository: &Repository,
        snapshot: &Snapshot,
        file: &ChangedFile,
    ) -> Result<Vec<u8>> {
        let mut arguments = vec![
            OsString::from("diff"),
            OsString::from("--no-ext-diff"),
            OsString::from("--no-textconv"),
            OsString::from("--find-renames"),
            OsString::from(snapshot.identity.review_unit().as_str()),
            OsString::from(snapshot.identity.snapshot_id()),
            OsString::from("--"),
        ];
        arguments.extend(file.diff_paths().map(|path| path.as_os_str().to_owned()));
        Ok(self.run(repository, arguments)?.stdout)
    }

    fn file_at(&self, repository: &Repository, revision: &str, path: &RepoPath) -> Result<Vec<u8>> {
        let mut object = OsString::from(revision);
        object.push(":");
        object.push(path.as_os_str());
        Ok(self
            .run(repository, [OsString::from("show"), object])?
            .stdout)
    }

    fn base_file_at(
        &self,
        repository: &Repository,
        snapshot: &Snapshot,
        path: &RepoPath,
    ) -> Result<Vec<u8>> {
        self.file_at(repository, snapshot.identity.review_unit().as_str(), path)
    }

    fn interdiff(
        &self,
        repository: &Repository,
        baseline_snapshot_id: &str,
        snapshot: &Snapshot,
        path: &RepoPath,
    ) -> Result<Interdiff> {
        let baseline = self.output(
            repository,
            [
                "cat-file",
                "-e",
                &format!("{baseline_snapshot_id}^{{tree}}"),
            ],
        )?;
        if !baseline.status.success() {
            return Ok(Interdiff::MissingBaseline);
        }
        Ok(Interdiff::Diff(
            self.run(
                repository,
                [
                    OsString::from("diff"),
                    OsString::from("--no-ext-diff"),
                    OsString::from("--no-textconv"),
                    OsString::from(baseline_snapshot_id),
                    OsString::from(snapshot.identity.snapshot_id()),
                    OsString::from("--"),
                    path.as_os_str().to_owned(),
                ],
            )?
            .stdout,
        ))
    }

    fn compare_baselines(
        &self,
        repository: &Repository,
        snapshot: &Snapshot,
        plan: &BaselineComparisonPlan,
    ) -> Result<BaselineComparisonResults> {
        let mut results = BaselineComparisonResults::default();
        for (baseline, paths) in plan.baselines() {
            let mut arguments = vec![
                OsString::from("diff"),
                OsString::from("--numstat"),
                OsString::from("-z"),
                OsString::from("--no-ext-diff"),
                OsString::from("--no-textconv"),
                OsString::from("--find-renames"),
                OsString::from(baseline.as_str()),
                OsString::from(snapshot.identity.snapshot_id()),
                OsString::from("--"),
            ];
            arguments.extend(paths.iter().map(git_literal_pathspec));
            let output = self.output(repository, arguments)?;
            if !output.status.success() {
                if !self.baseline_exists(repository, baseline.as_str())? {
                    results.insert(baseline.clone(), BaselineComparison::Missing);
                    continue;
                }
                return Err(Error::CommandFailed {
                    operation: "read Git repository".to_owned(),
                    code: output.status.code(),
                });
            }
            let path_statistics = ChangedFile::parse_git_stats(&output.stdout)?;
            results.insert(
                baseline.clone(),
                BaselineComparison::Compared { path_statistics },
            );
        }
        Ok(results)
    }

    // A Git working tree has no revisions for the reviewer to select or edit.
    fn revision_candidates(
        &self,
        _repository: &Repository,
        _direction: RevisionDirection,
    ) -> Result<Vec<RevisionCandidate>> {
        Ok(Vec::new())
    }

    fn revision_history(&self, _repository: &Repository) -> Result<Vec<RevisionHistoryLine>> {
        Ok(Vec::new())
    }

    fn identity_of(
        &self,
        _repository: &Repository,
        _change_id: &ChangeId,
    ) -> Result<Option<SnapshotIdentity>> {
        Ok(None)
    }

    fn edit_revision(&self, _repository: &Repository, _change_id: &ChangeId) -> Result<bool> {
        Ok(false)
    }
}

impl GitBackend {
    fn baseline_exists(&self, repository: &Repository, baseline: &str) -> Result<bool> {
        Ok(self
            .output(
                repository,
                ["cat-file", "-e", &format!("{baseline}^{{tree}}")],
            )?
            .status
            .success())
    }
}

fn git_literal_pathspec(path: &RepoPath) -> OsString {
    let mut pathspec = b":(top,literal)".to_vec();
    pathspec.extend_from_slice(path.as_bytes());
    OsString::from_vec(pathspec)
}

fn parse_tree_id(output: &[u8], detail: &'static str) -> Result<String> {
    String::from_utf8(trim_line_ending(output).to_vec()).map_err(|_| Error::Protocol {
        operation: "snapshot Git repository".to_owned(),
        detail,
    })
}

fn git_environment(cache: &GitCache) -> Vec<(OsString, OsString)> {
    vec![
        (
            OsString::from("GIT_INDEX_FILE"),
            cache.index.as_os_str().to_owned(),
        ),
        (
            OsString::from("GIT_OBJECT_DIRECTORY"),
            cache.objects.as_os_str().to_owned(),
        ),
        (
            OsString::from("GIT_ALTERNATE_OBJECT_DIRECTORIES"),
            cache.repository_objects.as_os_str().to_owned(),
        ),
    ]
}

impl Drop for GitCache {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.index);
        let mut lock = self.index.as_os_str().to_owned();
        lock.push(".lock");
        let _ = std::fs::remove_file(lock);
    }
}

#[cfg(test)]
#[path = "git.tests.rs"]
mod tests;
