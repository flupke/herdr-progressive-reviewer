//! Git working-tree snapshots backed by a private index and object store.

use std::ffi::{OsStr, OsString};
use std::fmt::Write as _;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};

use super::{
    ChangedFile, Interdiff, RepoPath, Repository, RepositoryBackend, RepositoryProcess, Snapshot,
    SnapshotIdentity, trim_line_ending,
};
use crate::{Error, Result};

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
    pub(super) fn new(repository_objects: PathBuf) -> Self {
        Self {
            repository_objects,
            cache: Mutex::new(None),
        }
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
            base_tree,
            snapshot_tree,
        })
    }
}

impl RepositoryBackend for GitBackend {
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
                        identity.review_id(),
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
                        identity.review_id(),
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
            OsString::from(snapshot.identity.review_id()),
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
        self.file_at(repository, snapshot.identity.review_id(), path)
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
