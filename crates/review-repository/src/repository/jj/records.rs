//! Parsers for the records printed by the jj templates.

use std::collections::HashMap;

use crate::repository::{
    ChangeId, ChangeKind, ChangedFile, DiffStatistics, FileKind, RepoPath, SnapshotId,
    SnapshotIdentity,
};
use crate::{Error, Result};

impl FileKind {
    fn from_jj(value: &[u8]) -> Result<Self> {
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

impl ChangedFile {
    /// Parse the changed-file records printed by `jj diff` with the file template.
    pub(super) fn parse_jj(output: &[u8]) -> Result<Vec<Self>> {
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
            files.push(Self::parse_jj_record(fields)?);
        }
        Self::sort_by_review_path(&mut files);
        Ok(files)
    }

    fn parse_jj_record(fields: &[&[u8]]) -> Result<Self> {
        let old_kind = FileKind::from_jj(fields[2])?;
        let new_kind = FileKind::from_jj(fields[3])?;
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

    /// Apply the per-path line counts printed by the jj statistics template.
    pub(super) fn add_jj_stats(files: &mut [Self], output: &[u8]) -> Result<()> {
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
}

impl SnapshotIdentity {
    /// Parse the record printed by the jj identity template.
    pub(super) fn parse_jj(output: &[u8]) -> Result<Self> {
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
            change_id: ChangeId::from(change_id.to_owned()),
            snapshot_id: SnapshotId::from(commit_id.to_owned()),
            description: description.to_owned(),
            display_id: String::from_utf8(raw_fields[3].to_vec()).map_err(|_| Error::Protocol {
                operation: "read jj snapshot identity".to_owned(),
                detail: "jj returned a non-UTF-8 display ID",
            })?,
        })
    }
}
