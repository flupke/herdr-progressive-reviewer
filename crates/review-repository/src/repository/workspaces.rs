//! What the workspaces of one repository share, and which workspaces and reviews the repository
//! has: the review state of a repository is keyed on its shared store, and the states that
//! each workspace kept for itself before are found by these (ADR 0013).

use std::path::PathBuf;

use review_types::ReviewUnit;

use super::git::GitBackend;
use super::watch::resolve_directory_or_link_file;
use super::{RepoType, Repository, RepositoryProcess};
use crate::{Error, Result};

/// How many reviews one jj command asks about, so its revset stays far below the 128 KiB that
/// Linux allows one argument.
const UNITS_PER_QUERY: usize = 512;

impl Repository {
    /// The store that every workspace of this repository shares: the canonical directory that
    /// `.jj/repo` resolves to in a jj workspace (a directory in the main workspace, a link file
    /// in the others), or the common directory of a Git worktree.
    pub fn shared_store(&self) -> Result<PathBuf> {
        let store = match self.repo_type() {
            RepoType::Jj => resolve_directory_or_link_file(&self.root.join(".jj/repo")),
            RepoType::Git => GitBackend::git_directory(&self.root)
                .map(|git| resolve_directory_or_link_file(&git.join("commondir")).unwrap_or(git)),
        };
        store.ok_or_else(|| Error::NotRepository {
            path: self.root.clone(),
        })
    }

    /// The canonical root of every workspace of this repository that still exists: its jj
    /// workspaces, or its Git worktrees.
    pub fn workspace_roots(&self) -> Result<Vec<PathBuf>> {
        let output = match self.repo_type() {
            RepoType::Jj => self.run_jj([
                "--ignore-working-copy",
                "workspace",
                "list",
                "-T",
                r#"self.root() ++ "\0""#,
            ])?,
            RepoType::Git => self.run_plain_git(&["worktree", "list", "--porcelain", "-z"], b"")?,
        };
        let roots = output
            .stdout
            .split(|byte| *byte == 0)
            .filter_map(|field| match self.repo_type() {
                RepoType::Jj => Some(field),
                RepoType::Git => field.strip_prefix(b"worktree "),
            })
            .filter(|path| !path.is_empty())
            .filter_map(|path| {
                let path = std::str::from_utf8(path).ok()?;
                std::fs::canonicalize(self.root.join(path)).ok()
            })
            .collect();
        Ok(roots)
    }

    /// Whether the repository holds one of `units`: a jj change, hidden ones included, or a
    /// Git tree. A unit that cannot name one in this kind of repository counts as missing, and
    /// so does jj's root change, which every jj repository holds.
    pub fn holds_any_review_unit<'a>(
        &self,
        units: impl IntoIterator<Item = &'a ReviewUnit>,
    ) -> Result<bool> {
        let units = units.into_iter().map(ReviewUnit::as_str);
        match self.repo_type() {
            RepoType::Jj => self.holds_any_change(
                &units
                    .filter(|unit| in_alphabet(unit, |byte| (b'k'..=b'z').contains(&byte)))
                    .collect::<Vec<_>>(),
            ),
            RepoType::Git => self.holds_any_tree(
                &units
                    .filter(|unit| {
                        in_alphabet(unit, |byte| {
                            byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
                        })
                    })
                    .collect::<Vec<_>>(),
            ),
        }
    }

    fn holds_any_change(&self, changes: &[&str]) -> Result<bool> {
        for chunk in changes.chunks(UNITS_PER_QUERY) {
            // `<change>/0` finds a hidden change too, where a bare change ID does not.
            let present = chunk
                .iter()
                .map(|change| format!("present({change}/0)"))
                .collect::<Vec<_>>()
                .join(" | ");
            let revset = format!("({present}) ~ root()");
            let output = self.run_jj([
                "--ignore-working-copy",
                "log",
                "--no-graph",
                "--limit",
                "1",
                "-r",
                &revset,
                "-T",
                r#"change_id ++ "\n""#,
            ])?;
            if !output.stdout.is_empty() {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn holds_any_tree(&self, trees: &[&str]) -> Result<bool> {
        if trees.is_empty() {
            return Ok(false);
        }
        let mut input = trees.join("\n");
        input.push('\n');
        let output = self.run_plain_git(
            &["cat-file", "--batch-check=%(objecttype)"],
            input.as_bytes(),
        )?;
        Ok(output
            .stdout
            .split(|byte| *byte == b'\n')
            .any(|line| line == b"tree"))
    }

    /// Run Git in the worktree with the repository's own index, reading `input`.
    fn run_plain_git(&self, arguments: &[&str], input: &[u8]) -> Result<std::process::Output> {
        let output =
            RepositoryProcess::new("git", &self.root, "read Git repository", &self.cancellation)
                .with_input(input)
                .output(arguments)?;
        if !output.status.success() {
            return Err(Error::CommandFailed {
                operation: "read Git repository".to_owned(),
                code: output.status.code(),
            });
        }
        Ok(output)
    }
}

/// Whether `unit` is not empty and every byte of it is in the alphabet `accepts` draws.
fn in_alphabet(unit: &str, accepts: impl Fn(u8) -> bool) -> bool {
    !unit.is_empty() && unit.bytes().all(accepts)
}

#[cfg(test)]
#[path = "workspaces.tests.rs"]
mod tests;
