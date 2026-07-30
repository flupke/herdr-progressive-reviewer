//! Stable snapshots of one jj working-copy change.

use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::{Error, Result};

const IDENTITY_TEMPLATE: &str = r#"change_id ++ "\0" ++ commit_id ++ "\0""#;
const FILE_TEMPLATE: &str = concat!(
    r#"source.path() ++ "\0" ++ target.path() ++ "\0" ++ "#,
    r#"source.file_type() ++ "\0" ++ target.file_type() ++ "\0" ++ "#,
    r#"status ++ "\0""#,
);

/// A full stable jj change identifier.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ChangeId(String);

/// A full jj commit identifier.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct CommitId(String);

impl CommitId {
    fn as_str(&self) -> &str {
        &self.0
    }
}

/// A lossless Unix repository-relative path.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RepoPath(Vec<u8>);

impl RepoPath {
    pub(crate) fn from_bytes(bytes: impl Into<Vec<u8>>) -> Self {
        Self(bytes.into())
    }

    fn as_os_str(&self) -> &OsStr {
        OsStr::from_bytes(&self.0)
    }

    fn display(&self) -> String {
        self.0
            .iter()
            .flat_map(|byte| std::ascii::escape_default(*byte))
            .map(char::from)
            .collect()
    }
}

/// The repository entry type on one side of a change.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileKind {
    /// No entry exists on this side.
    Absent,
    /// A regular file exists.
    File,
    /// A symbolic link exists.
    Symlink,
    /// The entry contains a merge conflict.
    Conflict,
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
}

/// The normalized change for one file row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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

/// One changed path in the current jj change.
#[derive(Clone, Debug, Eq, PartialEq)]
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
}

impl ChangedFile {
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
        files.sort_by(|left, right| left.display_path.cmp(&right.display_path));
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

        let change = if old_kind == FileKind::Conflict || new_kind == FileKind::Conflict {
            ChangeKind::Conflict
        } else if old_kind != FileKind::Absent
            && new_kind != FileKind::Absent
            && old_kind != new_kind
        {
            ChangeKind::TypeChanged
        } else {
            match status {
                "added" | "copied" => ChangeKind::Added,
                "modified" => ChangeKind::Modified,
                "removed" => ChangeKind::Deleted,
                "renamed" => ChangeKind::Renamed,
                _ => {
                    return Err(Error::Protocol {
                        operation: "read jj changed files".to_owned(),
                        detail: "jj returned an unknown file status",
                    });
                }
            }
        };

        let display_path = match (&old_path, &new_path) {
            (Some(old), Some(new)) if old != new => {
                format!("{} => {}", old.display(), new.display())
            }
            (Some(path), _) | (_, Some(path)) => path.display(),
            (None, None) => {
                return Err(Error::Protocol {
                    operation: "read jj changed files".to_owned(),
                    detail: "jj returned a changed file without a path",
                });
            }
        };

        Ok(Self {
            old_path,
            new_path,
            old_kind,
            new_kind,
            change,
            display_path,
        })
    }

    fn diff_paths(&self) -> impl Iterator<Item = &RepoPath> {
        self.old_path.iter().chain(
            self.new_path
                .iter()
                .filter(|new_path| self.old_path.as_ref() != Some(*new_path)),
        )
    }
}

/// The exact identity of one jj working-copy snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotIdentity {
    /// The stable change identifier.
    pub change_id: ChangeId,
    /// The exact commit identifier for this snapshot.
    pub commit_id: CommitId,
}

impl SnapshotIdentity {
    fn parse(output: &[u8]) -> Result<Self> {
        let fields: Vec<_> = output.split(|byte| *byte == 0).collect();
        if fields.len() != 3
            || fields[0].is_empty()
            || fields[1].is_empty()
            || !fields[2].is_empty()
        {
            return Err(Error::Protocol {
                operation: "read jj snapshot identity".to_owned(),
                detail: "jj returned an invalid identity record",
            });
        }
        let change_id = std::str::from_utf8(fields[0]).map_err(|_| Error::Protocol {
            operation: "read jj snapshot identity".to_owned(),
            detail: "jj returned a non-UTF-8 change ID",
        })?;
        let commit_id = std::str::from_utf8(fields[1]).map_err(|_| Error::Protocol {
            operation: "read jj snapshot identity".to_owned(),
            detail: "jj returned a non-UTF-8 commit ID",
        })?;

        Ok(Self {
            change_id: ChangeId(change_id.to_owned()),
            commit_id: CommitId(commit_id.to_owned()),
        })
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

/// A discovered jj workspace.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Repository {
    root: PathBuf,
}

impl Repository {
    /// Find the canonical jj workspace root for a directory.
    pub fn discover(start: impl AsRef<Path>) -> Result<Self> {
        let start = start.as_ref();
        let output = Command::new("jj")
            .args(["--color=never", "--no-pager", "root"])
            .current_dir(start)
            .output()
            .map_err(|source| Error::Spawn {
                operation: "discover jj repository".to_owned(),
                program: OsString::from("jj"),
                current_dir: Some(start.to_owned()),
                source,
            })?;
        if !output.status.success() {
            return Err(Error::NotJjRepository {
                path: start.to_owned(),
            });
        }

        let root = trim_line_ending(&output.stdout);
        if root.is_empty() {
            return Err(Error::Protocol {
                operation: "discover jj repository".to_owned(),
                detail: "jj root returned an empty path",
            });
        }

        Ok(Self {
            root: PathBuf::from(OsString::from_vec(root.to_vec())),
        })
    }

    /// Build one complete snapshot or reject a mixed poll.
    pub fn poll(&self) -> Result<PollResult> {
        self.run(["status"])?;
        let identity = self.read_identity(false)?;
        let files = self.read_files(&identity.commit_id)?;
        let verified = self.read_identity(true)?;

        if identity.commit_id != verified.commit_id {
            return Ok(PollResult::ChangedDuringPoll);
        }

        Ok(PollResult::Complete(Snapshot { identity, files }))
    }

    /// Read and parse the full Git-style diff for one changed file.
    pub fn diff(&self, snapshot: &Snapshot, file: &ChangedFile) -> Result<Vec<u8>> {
        let mut arguments = vec![
            OsString::from("diff"),
            OsString::from("-r"),
            OsString::from(snapshot.identity.commit_id.as_str()),
            OsString::from("--git"),
            OsString::from("--"),
        ];
        arguments.extend(file.diff_paths().map(|path| path.as_os_str().to_owned()));
        Ok(self.run(arguments)?.stdout)
    }

    fn read_identity(&self, ignore_working_copy: bool) -> Result<SnapshotIdentity> {
        let mut arguments = Vec::new();
        if ignore_working_copy {
            arguments.push(OsString::from("--ignore-working-copy"));
        }
        arguments.extend([
            OsString::from("log"),
            OsString::from("--no-graph"),
            OsString::from("-r"),
            OsString::from("@"),
            OsString::from("-T"),
            OsString::from(IDENTITY_TEMPLATE),
        ]);
        SnapshotIdentity::parse(&self.run(arguments)?.stdout)
    }

    fn read_files(&self, commit_id: &CommitId) -> Result<Vec<ChangedFile>> {
        let output = self.run([
            OsString::from("diff"),
            OsString::from("-r"),
            OsString::from(commit_id.as_str()),
            OsString::from("-T"),
            OsString::from(FILE_TEMPLATE),
        ])?;
        ChangedFile::parse_all(&output.stdout)
    }

    fn run<I, S>(&self, arguments: I) -> Result<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = Command::new("jj")
            .args(["--color=never", "--no-pager"])
            .args(arguments)
            .current_dir(&self.root)
            .output()
            .map_err(|source| Error::Spawn {
                operation: "read jj repository".to_owned(),
                program: OsString::from("jj"),
                current_dir: Some(self.root.clone()),
                source,
            })?;
        if !output.status.success() {
            return Err(Error::CommandFailed {
                operation: "read jj repository".to_owned(),
                code: output.status.code(),
            });
        }
        Ok(output)
    }
}

fn trim_line_ending(bytes: &[u8]) -> &[u8] {
    bytes
        .strip_suffix(b"\r\n")
        .or_else(|| bytes.strip_suffix(b"\n"))
        .unwrap_or(bytes)
}

#[cfg(test)]
mod tests {
    use super::RepoPath;

    #[test]
    fn repository_paths_preserve_non_utf8_bytes() {
        let path = RepoPath::from_bytes(b"invalid-\xff.txt");

        assert_eq!(path.0, b"invalid-\xff.txt");
        assert_eq!(path.display(), r"invalid-\xff.txt");
    }
}
