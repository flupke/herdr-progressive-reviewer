//! The hunks of one file: open ones in its open diff, and reviewed ones the
//! open diff no longer shows.

use std::ops::Range;

use review_repository::diff::DiffRow;

/// The changed lines of one hunk, as zero-based line ranges on each side.
/// Unchanged lines between the first and the last change belong to the span;
/// the context around it does not.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct HunkSpan {
    /// The lines the hunk replaces on the old side.
    pub old: Range<u32>,
    /// The lines the hunk puts on the current side.
    pub new: Range<u32>,
}

/// One hunk to accept or reopen.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HunkMark {
    /// Accept an open hunk: its old side is the reviewed version.
    Review(HunkSpan),
    /// Reopen a reviewed hunk: its old side is the base.
    Unreview(HunkSpan),
}

/// One hunk of the open diff, from the reviewed version to the current file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenHunk {
    /// The hunk's changed lines.
    pub span: HunkSpan,
    /// The hunk rewrites lines the reviewer already reviewed, so it shows
    /// only what changed since then.
    pub since_review: bool,
}

/// A change from the base that the reviewed version holds and the current
/// file still matches.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewedHunk {
    /// The change's lines: the base on the old side, the current file on the
    /// new side.
    pub span: HunkSpan,
    /// The change as diff rows, numbered like the base and the current file.
    pub rows: Vec<DiffRow>,
}

/// Which hunks of one file are open and which are reviewed.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FileHunks {
    /// The open diff's hunks, in the order of its hunk headers.
    pub open: Vec<OpenHunk>,
    /// The reviewed hunks, in file order.
    pub reviewed: Vec<ReviewedHunk>,
}

/// How many of a file's hunks are reviewed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HunkCount {
    /// Reviewed hunks.
    pub reviewed: usize,
    /// Reviewed and open hunks.
    pub total: usize,
}

impl FileHunks {
    /// The hunks of a file without reviewed lines: every hunk of its diff is open.
    pub fn unreviewed(rows: &[DiffRow]) -> Self {
        Self {
            open: open_spans(rows)
                .into_iter()
                .map(|span| OpenHunk {
                    span,
                    since_review: false,
                })
                .collect(),
            reviewed: Vec::new(),
        }
    }

    /// How many hunks are reviewed, once at least one is.
    pub fn count(&self) -> Option<HunkCount> {
        (!self.reviewed.is_empty()).then(|| HunkCount {
            reviewed: self.reviewed.len(),
            total: self.reviewed.len() + self.open.len(),
        })
    }
}

/// The spans of a parsed diff's hunks. A diff with notices (binary,
/// conflicted or unparsed content) has no hunks to review one by one.
pub(crate) fn open_spans(rows: &[DiffRow]) -> Vec<HunkSpan> {
    if rows.iter().any(|row| matches!(row, DiffRow::Notice { .. })) {
        return Vec::new();
    }
    let mut spans = Vec::new();
    let mut walk: Option<HunkWalk> = None;
    for row in rows {
        if let Some(started) = HunkWalk::start(row) {
            spans.extend(walk.replace(started).and_then(HunkWalk::finish));
        } else if let Some(walk) = walk.as_mut() {
            walk.push(row);
        }
    }
    spans.extend(walk.and_then(HunkWalk::finish));
    spans
}

/// A position on both sides of a diff.
#[derive(Clone, Copy)]
struct Position {
    old: u32,
    new: u32,
}

/// The walk through one hunk's rows.
struct HunkWalk {
    at: Position,
    first: Option<Position>,
    last: Option<Position>,
}

impl HunkWalk {
    fn start(row: &DiffRow) -> Option<Self> {
        let DiffRow::Hunk {
            old_start,
            old_count,
            new_start,
            new_count,
        } = *row
        else {
            return None;
        };
        // A side without lines names the line before the hunk.
        let first_line = |start: u32, count: u32| if count == 0 { start } else { start - 1 };
        Some(Self {
            at: Position {
                old: first_line(old_start, old_count),
                new: first_line(new_start, new_count),
            },
            first: None,
            last: None,
        })
    }

    fn push(&mut self, row: &DiffRow) {
        let (old, new) = match row {
            DiffRow::Context { .. } => (1, 1),
            DiffRow::Delete { .. } => (1, 0),
            DiffRow::Add { .. } => (0, 1),
            _ => return,
        };
        let changed = old + new == 1;
        if changed {
            self.first.get_or_insert(self.at);
        }
        self.at.old += old;
        self.at.new += new;
        if changed {
            self.last = Some(self.at);
        }
    }

    fn finish(self) -> Option<HunkSpan> {
        let (first, last) = (self.first?, self.last?);
        Some(HunkSpan {
            old: first.old..last.old,
            new: first.new..last.new,
        })
    }
}

#[cfg(test)]
#[path = "hunks.tests.rs"]
mod tests;
