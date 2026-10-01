//! Telling a file's open hunks from its reviewed ones.

use std::ops::Range;

use review_repository::diff::DiffRow;

use crate::hunks::{FileHunks, HunkSpan, OpenHunk, ReviewedHunk, open_spans};
use crate::review::HunkReview;
use crate::text::{CONTEXT, Change, Lines, changes, overlaps, translate};

/// A reviewed change placed on all three versions.
struct Placed {
    base: Range<u32>,
    reviewed: Range<u32>,
    current: Range<u32>,
}

impl HunkReview<'_> {
    /// Which hunks are open and which are reviewed, given the parsed open diff
    /// (reviewed version to current file). An open hunk that meets a change
    /// of the reviewed version rewrites reviewed lines. A change of the
    /// reviewed version that no open change touches is a reviewed hunk, and
    /// nearby ones join into one hunk as Git joins hunks.
    pub fn hunks(&self, open_rows: &[DiffRow]) -> FileHunks {
        if open_rows
            .iter()
            .any(|row| matches!(row, DiffRow::Notice { .. }))
        {
            return FileHunks::default();
        }
        let reviewed = changes(self.base, self.reviewed);
        let open = changes(self.reviewed, self.current);
        FileHunks {
            open: open_spans(open_rows)
                .into_iter()
                .map(|span| OpenHunk {
                    since_review: rewrites_reviewed_lines(&span, &open, &reviewed),
                    span,
                })
                .collect(),
            reviewed: self.reviewed_hunks(&reviewed, &open),
        }
    }

    fn reviewed_hunks(&self, reviewed: &[Change], open: &[Change]) -> Vec<ReviewedHunk> {
        let rows = HunkRows {
            base: Lines::new(self.base),
            current: Lines::new(self.current),
        };
        let mut hunks = Vec::new();
        let mut group: Vec<Placed> = Vec::new();
        for change in reviewed {
            let placed = translate(
                open.iter().map(|change| (&change.before, &change.after)),
                &change.after,
            )
            .map(|current| Placed {
                base: change.before.clone(),
                reviewed: change.after.clone(),
                current,
            });
            let joins = placed
                .as_ref()
                .zip(group.last())
                .is_some_and(|(next, last)| {
                    next.base.start - last.base.end <= 2 * CONTEXT
                        && !open.iter().any(|change| {
                            overlaps(&change.before, &(last.reviewed.end..next.reviewed.start))
                        })
                });
            if !joins {
                hunks.extend(rows.hunk(&std::mem::take(&mut group)));
            }
            group.extend(placed);
        }
        hunks.extend(rows.hunk(&group));
        hunks
    }
}

/// Whether one of the open hunk's own changes meets a reviewed change; the
/// unchanged lines between its changes may hold reviewed lines untouched.
fn rewrites_reviewed_lines(span: &HunkSpan, open: &[Change], reviewed: &[Change]) -> bool {
    open.iter()
        .filter(|change| overlaps(&change.before, &span.old))
        .any(|change| {
            reviewed
                .iter()
                .any(|reviewed| overlaps(&reviewed.after, &change.before))
        })
}

/// The base and current lines a reviewed hunk's rows show.
struct HunkRows<'a> {
    base: Lines<'a>,
    current: Lines<'a>,
}

impl HunkRows<'_> {
    fn hunk(&self, group: &[Placed]) -> Option<ReviewedHunk> {
        let (first, last) = (group.first()?, group.last()?);
        let mut rows = Vec::new();
        let mut previous: Option<&Placed> = None;
        for placed in group {
            if let Some(previous) = previous {
                for offset in 0..placed.current.start - previous.current.end {
                    let new_line = previous.current.end + offset;
                    rows.push(DiffRow::Context {
                        old_line: previous.base.end + offset + 1,
                        new_line: new_line + 1,
                        text: row_text(' ', self.current.line(new_line)),
                    });
                }
            }
            rows.extend(placed.base.clone().map(|line| DiffRow::Delete {
                old_line: line + 1,
                text: row_text('-', self.base.line(line)),
            }));
            rows.extend(placed.current.clone().map(|line| DiffRow::Add {
                new_line: line + 1,
                text: row_text('+', self.current.line(line)),
            }));
            previous = Some(placed);
        }
        Some(ReviewedHunk {
            span: HunkSpan {
                old: first.base.start..last.base.end,
                new: first.current.start..last.current.end,
            },
            rows,
        })
    }
}

/// A diff row's text: its marker and the line without its terminator.
fn row_text(marker: char, line: Option<&[u8]>) -> String {
    let line = line.unwrap_or_default();
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    format!("{marker}{}", String::from_utf8_lossy(line))
}

#[cfg(test)]
#[path = "classify.tests.rs"]
mod tests;
