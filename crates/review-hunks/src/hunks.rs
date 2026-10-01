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
    /// How many lines the hunk adds or removes.
    pub(crate) changed_lines: u32,
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

/// How many of a file's changed lines are reviewed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LineCount {
    /// Added and removed lines in reviewed hunks.
    pub reviewed: u32,
    /// Added and removed lines in reviewed and open hunks.
    pub total: u32,
}

impl LineCount {
    /// The reviewed share in whole percent: rounded down, but never 0% once
    /// a line is reviewed nor 100% while one is open.
    pub fn percent(self) -> u32 {
        let percent = u64::from(self.reviewed) * 100 / u64::from(self.total.max(1));
        let percent = u32::try_from(percent).unwrap_or(100);
        if self.reviewed == 0 || self.reviewed >= self.total {
            percent
        } else {
            percent.clamp(1, 99)
        }
    }
}

impl FileHunks {
    /// The hunks of a file without reviewed lines: every hunk of its diff is open.
    pub fn unreviewed(rows: &[DiffRow]) -> Self {
        Self {
            open: open_hunks(rows),
            reviewed: Vec::new(),
        }
    }

    /// Whether the file has no hunks to mark line by line, as binary and
    /// other non-text changes do.
    pub fn is_empty(&self) -> bool {
        self.open.is_empty() && self.reviewed.is_empty()
    }

    /// How many changed lines are reviewed, once some are.
    pub fn count(&self) -> Option<LineCount> {
        let reviewed = self
            .reviewed
            .iter()
            .flat_map(|hunk| &hunk.rows)
            .filter(|row| matches!(row, DiffRow::Add { .. } | DiffRow::Delete { .. }))
            .count();
        let reviewed = u32::try_from(reviewed).unwrap_or(u32::MAX);
        let open = self.open.iter().map(|hunk| hunk.changed_lines).sum::<u32>();
        (reviewed > 0).then_some(LineCount {
            reviewed,
            total: reviewed.saturating_add(open),
        })
    }
}

/// The hunks of a parsed diff, none of them rewriting reviewed lines yet. A
/// diff with notices (binary, conflicted or unparsed content) has no hunks to
/// review one by one.
pub(crate) fn open_hunks(rows: &[DiffRow]) -> Vec<OpenHunk> {
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
    changed: u32,
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
            changed: 0,
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
            self.changed += 1;
        }
        self.at.old += old;
        self.at.new += new;
        if changed {
            self.last = Some(self.at);
        }
    }

    fn finish(self) -> Option<OpenHunk> {
        let (first, last) = (self.first?, self.last?);
        Some(OpenHunk {
            span: HunkSpan {
                old: first.old..last.old,
                new: first.new..last.new,
            },
            since_review: false,
            changed_lines: self.changed,
        })
    }
}

#[cfg(test)]
#[path = "hunks.tests.rs"]
mod tests;
