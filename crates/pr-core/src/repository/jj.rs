//! Jujutsu change snapshots.

use std::ffi::OsString;
use std::path::Path;

use super::{
    ChangedFile, Interdiff, RepoPath, Repository, RepositoryBackend, Snapshot, SnapshotIdentity,
};
use crate::Result;

pub(super) const IDENTITY_TEMPLATE: &str =
    r#"change_id ++ "\0" ++ commit_id ++ "\0" ++ description ++ "\0""#;
const FILE_TEMPLATE: &str = concat!(
    r#"source.path() ++ "\0" ++ target.path() ++ "\0" ++ "#,
    r#"source.file_type() ++ "\0" ++ target.file_type() ++ "\0" ++ "#,
    r#"status ++ "\0""#,
);
const STATS_TEMPLATE: &str = concat!(
    r#"diff.stat().files().map(|entry| entry.path() ++ "\0" ++ "#,
    r#"entry.lines_added() ++ "\0" ++ entry.lines_removed() ++ "\0").join("")"#,
);
const DESCRIPTION_DIFF_HEADER: &[u8] =
    b"diff --git a/JJ-COMMIT-DESCRIPTION b/JJ-COMMIT-DESCRIPTION\n";
const DESCRIPTION_DIFF_PATHS: &[u8] = b"--- JJ-COMMIT-DESCRIPTION\n+++ JJ-COMMIT-DESCRIPTION\n";
const NEXT_DIFF_HEADER: &[u8] = b"\ndiff --git ";

#[derive(Debug)]
pub(super) struct JjBackend;

impl RepositoryBackend for JjBackend {
    fn set_state_root(&self, _repository_root: &Path, _state_root: &Path) {}

    fn current_identity(&self, repository: &Repository) -> Result<SnapshotIdentity> {
        repository.run_jj(["status"])?;
        repository.read_jj_identity(false)
    }

    fn read_files(
        &self,
        repository: &Repository,
        identity: &SnapshotIdentity,
    ) -> Result<Vec<ChangedFile>> {
        ChangedFile::parse_all(
            &repository
                .run_jj([
                    OsString::from("diff"),
                    OsString::from("-r"),
                    OsString::from(identity.snapshot_id()),
                    OsString::from("-T"),
                    OsString::from(FILE_TEMPLATE),
                ])?
                .stdout,
        )
    }

    fn read_stats(
        &self,
        repository: &Repository,
        identity: &SnapshotIdentity,
        files: &mut [ChangedFile],
    ) -> Result<()> {
        ChangedFile::add_stats(
            files,
            &repository
                .run_jj([
                    OsString::from("--ignore-working-copy"),
                    OsString::from("log"),
                    OsString::from("--no-graph"),
                    OsString::from("-r"),
                    OsString::from(identity.snapshot_id()),
                    OsString::from("-T"),
                    OsString::from(STATS_TEMPLATE),
                ])?
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
            OsString::from("-r"),
            OsString::from(snapshot.identity.snapshot_id()),
            OsString::from("--git"),
            OsString::from("--"),
        ];
        arguments.extend(file.diff_paths().map(|path| path.as_os_str().to_owned()));
        Ok(repository.run_jj(arguments)?.stdout)
    }

    fn file_at(&self, repository: &Repository, revision: &str, path: &RepoPath) -> Result<Vec<u8>> {
        Ok(repository
            .run_jj([
                OsString::from("--ignore-working-copy"),
                OsString::from("file"),
                OsString::from("show"),
                OsString::from("-r"),
                OsString::from(revision),
                OsString::from("--"),
                path.as_os_str().to_owned(),
            ])?
            .stdout)
    }

    fn base_file_at(
        &self,
        repository: &Repository,
        snapshot: &Snapshot,
        path: &RepoPath,
    ) -> Result<Vec<u8>> {
        self.file_at(
            repository,
            &format!("{}-", snapshot.identity.snapshot_id()),
            path,
        )
    }

    fn interdiff(
        &self,
        repository: &Repository,
        baseline_snapshot_id: &str,
        snapshot: &Snapshot,
        path: &RepoPath,
    ) -> Result<Interdiff> {
        let baseline = repository.output_jj([
            OsString::from("--ignore-working-copy"),
            OsString::from("log"),
            OsString::from("--no-graph"),
            OsString::from("-r"),
            OsString::from(baseline_snapshot_id),
            OsString::from("-T"),
            OsString::from(r#"commit_id ++ "\n""#),
        ])?;
        if !baseline.status.success() {
            return Ok(Interdiff::MissingBaseline);
        }
        Ok(Interdiff::Diff(strip_description_diff(
            repository
                .run_jj([
                    OsString::from("--ignore-working-copy"),
                    OsString::from("interdiff"),
                    OsString::from("--from"),
                    OsString::from(baseline_snapshot_id),
                    OsString::from("--to"),
                    OsString::from(snapshot.identity.snapshot_id()),
                    OsString::from("--git"),
                    OsString::from("--"),
                    path.as_os_str().to_owned(),
                ])?
                .stdout,
        )))
    }
}

fn strip_description_diff(mut diff: Vec<u8>) -> Vec<u8> {
    if !diff.starts_with(DESCRIPTION_DIFF_HEADER) {
        return diff;
    }

    let block_end = diff[DESCRIPTION_DIFF_HEADER.len()..]
        .windows(NEXT_DIFF_HEADER.len())
        .position(|window| window == NEXT_DIFF_HEADER)
        .map_or(diff.len(), |index| {
            index + DESCRIPTION_DIFF_HEADER.len() + 1
        });
    if diff[..block_end]
        .windows(DESCRIPTION_DIFF_PATHS.len())
        .any(|window| window == DESCRIPTION_DIFF_PATHS)
    {
        diff.drain(..block_end);
    }
    diff
}
