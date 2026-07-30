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
    /// The entry is a Git submodule.
    Submodule,
    /// The parser did not recognize the row or the hunk shape.
    Unsupported,
}

/// Parse the diff for a known file and preserve its lossless paths.
pub fn parse_file_diff(output: &[u8], file: &ChangedFile) -> Vec<DiffRow> {
    if file.old_kind == FileKind::Submodule || file.new_kind == FileKind::Submodule {
        return vec![DiffRow::Notice {
            kind: NoticeKind::Submodule,
            text: "Git submodule content changed".to_owned(),
        }];
    }
    if file.old_kind == FileKind::Symlink || file.new_kind == FileKind::Symlink {
        return vec![DiffRow::Notice {
            kind: NoticeKind::Unsupported,
            text: "Symbolic link target changed; text diff is unavailable".to_owned(),
        }];
    }

    let mut rows = parse_unified_diff(output);
    for row in &mut rows {
        if let DiffRow::FileHeader { old_path, new_path } = row {
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

/// Parse Git-style unified diff output without panicking on unknown rows.
fn parse_unified_diff(output: &[u8]) -> Vec<DiffRow> {
    let Ok(text) = std::str::from_utf8(output) else {
        return vec![DiffRow::Notice {
            kind: NoticeKind::Binary,
            text: "Binary file; text diff is unavailable".to_owned(),
        }];
    };

    let mut rows = Vec::new();
    let mut hunk = None;
    for line in text.lines() {
        if line.starts_with("diff --git ") {
            finish_hunk(&mut rows, hunk.take());
            rows.push(DiffRow::FileHeader {
                old_path: None,
                new_path: None,
            });
        } else if let Some(parsed) = parse_hunk_header(line) {
            finish_hunk(&mut rows, hunk.replace(parsed));
            rows.push(DiffRow::Hunk {
                old_start: parsed.old_start,
                old_count: parsed.old_count,
                new_start: parsed.new_start,
                new_count: parsed.new_count,
            });
        } else if let Some(active) = hunk.as_mut() {
            parse_hunk_row(line, active, &mut rows);
        } else {
            rows.push(parse_metadata(line));
        }
    }
    finish_hunk(&mut rows, hunk);
    rows
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

fn parse_hunk_header(line: &str) -> Option<ActiveHunk> {
    let rest = line.strip_prefix("@@ -")?;
    let (old, rest) = rest.split_once(" +")?;
    let (new, _) = rest.split_once(" @@")?;
    let (old_start, old_count) = parse_range(old)?;
    let (new_start, new_count) = parse_range(new)?;
    Some(ActiveHunk {
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

fn parse_hunk_row(line: &str, hunk: &mut ActiveHunk, rows: &mut Vec<DiffRow>) {
    if line.starts_with("\\ No newline at end of file") {
        rows.push(DiffRow::Meta {
            text: line.to_owned(),
        });
    } else if is_conflict_marker(line) {
        rows.push(DiffRow::Notice {
            kind: NoticeKind::Conflict,
            text: line.to_owned(),
        });
        advance_for_marker(line.as_bytes().first().copied(), hunk);
    } else {
        match line.as_bytes().first().copied() {
            Some(b' ') => {
                rows.push(DiffRow::Context {
                    old_line: hunk.old_start + hunk.old_seen,
                    new_line: hunk.new_start + hunk.new_seen,
                    text: line.to_owned(),
                });
                hunk.old_seen += 1;
                hunk.new_seen += 1;
            }
            Some(b'-') => {
                rows.push(DiffRow::Delete {
                    old_line: hunk.old_start + hunk.old_seen,
                    text: line.to_owned(),
                });
                hunk.old_seen += 1;
            }
            Some(b'+') => {
                rows.push(DiffRow::Add {
                    new_line: hunk.new_start + hunk.new_seen,
                    text: line.to_owned(),
                });
                hunk.new_seen += 1;
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

fn advance_for_marker(marker: Option<u8>, hunk: &mut ActiveHunk) {
    match marker {
        Some(b' ') => {
            hunk.old_seen += 1;
            hunk.new_seen += 1;
        }
        Some(b'-') => hunk.old_seen += 1,
        Some(b'+') => hunk.new_seen += 1,
        _ => {}
    }
}

fn finish_hunk(rows: &mut Vec<DiffRow>, hunk: Option<ActiveHunk>) {
    if let Some(hunk) = hunk {
        if hunk.old_seen != hunk.old_count || hunk.new_seen != hunk.new_count {
            rows.push(DiffRow::Notice {
                kind: NoticeKind::Unsupported,
                text: "Unified diff hunk row count does not match its header".to_owned(),
            });
        }
    }
}

fn parse_metadata(line: &str) -> DiffRow {
    let kind = if line.starts_with("Binary files ") || line == "GIT binary patch" {
        Some(NoticeKind::Binary)
    } else if line.starts_with("Subproject commit ") || line.contains("mode 160000") {
        Some(NoticeKind::Submodule)
    } else {
        None
    };
    if let Some(kind) = kind {
        DiffRow::Notice {
            kind,
            text: match kind {
                NoticeKind::Binary => "Binary file; text diff is unavailable".to_owned(),
                NoticeKind::Submodule => "Git submodule content changed".to_owned(),
                NoticeKind::Conflict | NoticeKind::Unsupported => line.to_owned(),
            },
        }
    } else if is_known_metadata(line) {
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

fn is_known_metadata(line: &str) -> bool {
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
