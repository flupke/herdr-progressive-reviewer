//! Parsers for the raw and numstat records printed by `git diff -z`.

use std::collections::BTreeMap;

use crate::repository::{ChangeKind, ChangedFile, DiffStatistics, FileKind, RepoPath};
use crate::{Error, Result};

impl FileKind {
    fn from_git_mode(value: &[u8]) -> Result<Self> {
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
            old_kind: FileKind::from_git_mode(old_mode)?,
            new_kind: FileKind::from_git_mode(new_mode)?,
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
    /// Parse the records printed by `git diff --raw -z`.
    pub(super) fn parse_git(output: &[u8]) -> Result<Vec<Self>> {
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
        Self::sort_by_review_path(&mut files);
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

    /// Parse the per-path line counts printed by `git diff --numstat -z`.
    pub(super) fn parse_git_stats(output: &[u8]) -> Result<BTreeMap<RepoPath, DiffStatistics>> {
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

    /// Apply the line counts printed by `git diff --numstat -z`.
    pub(super) fn add_git_stats(files: &mut [Self], output: &[u8]) -> Result<()> {
        let statistics = Self::parse_git_stats(output)?;
        for file in files {
            if let Some(statistics) = statistics.get(file.review_path()) {
                file.statistics = *statistics;
            }
        }
        Ok(())
    }
}
