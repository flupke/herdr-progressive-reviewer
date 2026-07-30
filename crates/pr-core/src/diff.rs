//! Git-style unified diff rows.

use crate::repository::{ChangedFile, FileKind, RepoPath};

/// One parsed row in a unified diff.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DiffRow {
    /// The paths from a `diff --git` header.
    FileHeader {
        /// The parent-side path, when present.
        old_path: Option<RepoPath>,
        /// The current-side path, when present.
        new_path: Option<RepoPath>,
        /// The complete original row.
        text: String,
    },
    /// A file header or other known Git metadata row.
    Meta {
        /// The complete original row.
        text: String,
    },
    /// A unified diff hunk header.
    Hunk {
        /// The first parent-side line.
        old_start: u32,
        /// The number of parent-side lines.
        old_count: u32,
        /// The first current-side line.
        new_start: u32,
        /// The number of current-side lines.
        new_count: u32,
    },
    /// An unchanged row.
    Context {
        /// The parent-side line number.
        old_line: u32,
        /// The current-side line number.
        new_line: u32,
        /// The complete original row.
        text: String,
    },
    /// A deleted row.
    Delete {
        /// The parent-side line number.
        old_line: u32,
        /// The complete original row.
        text: String,
    },
    /// An added row.
    Add {
        /// The current-side line number.
        new_line: u32,
        /// The complete original row.
        text: String,
    },
    /// A special file or unsupported row.
    Notice {
        /// The type of notice.
        kind: NoticeKind,
        /// Text that explains the notice.
        text: String,
    },
}

/// A special condition in diff output.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NoticeKind {
    /// Text diff is not available for binary content.
    Binary,
    /// The file contains a merge conflict.
    Conflict,
    /// The parser did not recognize the row or the hunk shape.
    Unsupported,
}

#[derive(Debug)]
struct DiffParser {
    rows: Vec<DiffRow>,
    hunk: Option<ActiveHunk>,
}

#[derive(Clone, Copy, Debug)]
struct ActiveHunk {
    old_start: u32,
    old_count: u32,
    new_start: u32,
    new_count: u32,
    old_seen: u32,
    new_seen: u32,
}

impl DiffParser {
    fn parse(output: &[u8]) -> Vec<DiffRow> {
        let Ok(text) = std::str::from_utf8(output) else {
            return vec![DiffRow::Notice {
                kind: NoticeKind::Binary,
                text: "Binary file; text diff is unavailable".to_owned(),
            }];
        };

        let mut parser = Self {
            rows: Vec::new(),
            hunk: None,
        };
        for line in text.lines() {
            parser.push(line);
        }
        parser.finish_hunk();
        parser.rows
    }

    fn push(&mut self, line: &str) {
        if line.starts_with("diff --git ") {
            self.finish_hunk();
            self.rows.push(DiffRow::FileHeader {
                old_path: None,
                new_path: None,
                text: line.to_owned(),
            });
        } else if let Some(hunk) = ActiveHunk::parse(line) {
            self.finish_hunk();
            self.rows.push(hunk.header());
            self.hunk = Some(hunk);
        } else if let Some(hunk) = self.hunk.as_mut() {
            hunk.push(line, &mut self.rows);
        } else {
            self.rows.push(Self::metadata(line));
        }
    }

    fn finish_hunk(&mut self) {
        if self.hunk.take().is_some_and(|hunk| !hunk.is_complete()) {
            self.rows.push(DiffRow::Notice {
                kind: NoticeKind::Unsupported,
                text: "Unified diff hunk row count does not match its header".to_owned(),
            });
        }
    }

    fn metadata(line: &str) -> DiffRow {
        if line.starts_with("Binary files ") || line == "GIT binary patch" {
            DiffRow::Notice {
                kind: NoticeKind::Binary,
                text: "Binary file; text diff is unavailable".to_owned(),
            }
        } else if Self::is_metadata(line) {
            DiffRow::Meta {
                text: line.to_owned(),
            }
        } else {
            DiffRow::Notice {
                kind: NoticeKind::Unsupported,
                text: line.to_owned(),
            }
        }
    }

    fn is_metadata(line: &str) -> bool {
        line.starts_with("index ")
            || line.starts_with("--- ")
            || line.starts_with("+++ ")
            || line.starts_with("old mode ")
            || line.starts_with("new mode ")
            || line.starts_with("new file mode ")
            || line.starts_with("deleted file mode ")
            || line.starts_with("similarity index ")
            || line.starts_with("rename from ")
            || line.starts_with("rename to ")
            || line.is_empty()
    }
}

impl ActiveHunk {
    fn parse(line: &str) -> Option<Self> {
        let rest = line.strip_prefix("@@ -")?;
        let (old, rest) = rest.split_once(" +")?;
        let (new, _) = rest.split_once(" @@")?;
        let (old_start, old_count) = Self::parse_range(old)?;
        let (new_start, new_count) = Self::parse_range(new)?;
        Some(Self {
            old_start,
            old_count,
            new_start,
            new_count,
            old_seen: 0,
            new_seen: 0,
        })
    }

    fn parse_range(value: &str) -> Option<(u32, u32)> {
        match value.split_once(',') {
            Some((start, count)) => Some((start.parse().ok()?, count.parse().ok()?)),
            None => Some((value.parse().ok()?, 1)),
        }
    }

    fn header(self) -> DiffRow {
        DiffRow::Hunk {
            old_start: self.old_start,
            old_count: self.old_count,
            new_start: self.new_start,
            new_count: self.new_count,
        }
    }

    fn push(&mut self, line: &str, rows: &mut Vec<DiffRow>) {
        if line.starts_with("\\ No newline at end of file") {
            rows.push(DiffRow::Meta {
                text: line.to_owned(),
            });
        } else if Self::is_conflict_marker(line) {
            rows.push(DiffRow::Notice {
                kind: NoticeKind::Conflict,
                text: line.to_owned(),
            });
            self.advance(line.as_bytes().first().copied());
        } else {
            match line.as_bytes().first().copied() {
                Some(b' ') => {
                    rows.push(DiffRow::Context {
                        old_line: self.old_start + self.old_seen,
                        new_line: self.new_start + self.new_seen,
                        text: line.to_owned(),
                    });
                    self.old_seen += 1;
                    self.new_seen += 1;
                }
                Some(b'-') => {
                    rows.push(DiffRow::Delete {
                        old_line: self.old_start + self.old_seen,
                        text: line.to_owned(),
                    });
                    self.old_seen += 1;
                }
                Some(b'+') => {
                    rows.push(DiffRow::Add {
                        new_line: self.new_start + self.new_seen,
                        text: line.to_owned(),
                    });
                    self.new_seen += 1;
                }
                _ => rows.push(DiffRow::Notice {
                    kind: NoticeKind::Unsupported,
                    text: line.to_owned(),
                }),
            }
        }
    }

    fn is_conflict_marker(line: &str) -> bool {
        line.contains("<<<<<<< conflict")
            || line.contains("%%%%%%% diff from:")
            || line.contains(r"\\\\\\\        to:")
            || line.contains(">>>>>>> conflict")
    }

    fn advance(&mut self, marker: Option<u8>) {
        match marker {
            Some(b' ') => {
                self.old_seen += 1;
                self.new_seen += 1;
            }
            Some(b'-') => self.old_seen += 1,
            Some(b'+') => self.new_seen += 1,
            _ => {}
        }
    }

    fn is_complete(self) -> bool {
        self.old_seen == self.old_count && self.new_seen == self.new_count
    }
}

/// Parse the diff for a known file and preserve its lossless paths.
pub fn parse_file_diff(output: &[u8], file: &ChangedFile) -> Vec<DiffRow> {
    if file.old_kind == FileKind::Symlink || file.new_kind == FileKind::Symlink {
        return vec![DiffRow::Notice {
            kind: NoticeKind::Unsupported,
            text: "Symbolic link target changed; text diff is unavailable".to_owned(),
        }];
    }

    let mut rows = DiffParser::parse(output);
    for row in &mut rows {
        if let DiffRow::FileHeader {
            old_path, new_path, ..
        } = row
        {
            old_path.clone_from(&file.old_path);
            new_path.clone_from(&file.new_path);
        }
    }
    if file.old_kind == FileKind::Conflict || file.new_kind == FileKind::Conflict {
        let has_notice = rows.iter().any(|row| {
            matches!(
                row,
                DiffRow::Notice {
                    kind: NoticeKind::Conflict,
                    ..
                }
            )
        });
        if !has_notice {
            rows.insert(
                0,
                DiffRow::Notice {
                    kind: NoticeKind::Conflict,
                    text: "File contains an unresolved conflict".to_owned(),
                },
            );
        }
    }
    rows
}
