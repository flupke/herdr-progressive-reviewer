//! Minimal valid excerpts from parsed unified diffs.

use std::fmt::Write as _;
use std::ops::RangeInclusive;

use crate::diff::DiffRow;

/// A valid unified-diff excerpt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiffExcerpt(String);

impl DiffExcerpt {
    /// Build the smallest valid excerpt for an inclusive row selection.
    pub fn build(rows: &[DiffRow], selection: RangeInclusive<usize>) -> Result<Self, ExcerptError> {
        if rows.iter().any(|row| matches!(row, DiffRow::Notice { .. })) {
            return Err(ExcerptError::Notice);
        }

        let headers = FileHeaders::read(rows)?;
        let mut hunks = Vec::new();
        let mut index = 0;
        while index < rows.len() {
            let DiffRow::Hunk {
                old_start,
                old_count,
                new_start,
                new_count,
            } = rows[index]
            else {
                index += 1;
                continue;
            };
            let end = rows[index + 1..]
                .iter()
                .position(|row| matches!(row, DiffRow::Hunk { .. }))
                .map_or(rows.len(), |offset| index + 1 + offset);
            if let Some(hunk) = ExcerptHunk::read(
                &rows[index + 1..end],
                index + 1,
                &selection,
                (old_start, old_count),
                (new_start, new_count),
            ) {
                hunks.push(hunk);
            }
            index = end;
        }
        if hunks.is_empty() {
            return Err(ExcerptError::NoContent);
        }

        let mut text = headers.text;
        for hunk in hunks {
            write!(
                text,
                "\n@@ -{},{} +{},{} @@\n{}",
                hunk.old_start,
                hunk.old_count,
                hunk.new_start,
                hunk.new_count,
                hunk.lines.join("\n")
            )
            .expect("writing to a String cannot fail");
        }
        Ok(Self(text))
    }

    /// Get the excerpt text without a trailing newline.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume the excerpt and return its text.
    pub fn into_string(self) -> String {
        self.0
    }
}

/// Why a diff selection cannot become an excerpt.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ExcerptError {
    /// The diff does not contain the required file headers.
    #[error("Diff headers are incomplete")]
    MissingHeaders,
    /// The selection has no context, added, or deleted row.
    #[error("Select context, added, or deleted lines")]
    NoContent,
    /// The diff contains a binary, conflict, or unsupported notice.
    #[error("This diff cannot be inserted as text")]
    Notice,
}

#[derive(Debug)]
struct FileHeaders {
    text: String,
}

impl FileHeaders {
    fn read(rows: &[DiffRow]) -> Result<Self, ExcerptError> {
        let mut lines = Vec::new();
        for row in rows {
            match row {
                DiffRow::FileHeader { text, .. } => lines.push(text.as_str()),
                DiffRow::Meta { text } if !text.starts_with("index ") => lines.push(text),
                DiffRow::Hunk { .. } => break,
                _ => {}
            }
        }
        let complete = lines.iter().any(|line| line.starts_with("diff --git "))
            && lines.iter().any(|line| line.starts_with("--- "))
            && lines.iter().any(|line| line.starts_with("+++ "));
        if !complete {
            return Err(ExcerptError::MissingHeaders);
        }
        Ok(Self {
            text: lines.join("\n"),
        })
    }
}

#[derive(Debug)]
struct ExcerptHunk {
    old_start: u32,
    old_count: u32,
    new_start: u32,
    new_count: u32,
    lines: Vec<String>,
}

#[derive(Debug)]
struct HunkRow<'a> {
    row: &'a DiffRow,
    offset: usize,
    old_line: u32,
    new_line: u32,
    selected: bool,
}

impl HunkRow<'_> {
    fn is_context(&self) -> bool {
        matches!(self.row, DiffRow::Context { .. })
    }

    fn old_count(&self) -> u32 {
        u32::from(matches!(
            self.row,
            DiffRow::Context { .. } | DiffRow::Delete { .. }
        ))
    }

    fn new_count(&self) -> u32 {
        u32::from(matches!(
            self.row,
            DiffRow::Context { .. } | DiffRow::Add { .. }
        ))
    }

    fn text(&self) -> &str {
        match self.row {
            DiffRow::Context { text, .. }
            | DiffRow::Delete { text, .. }
            | DiffRow::Add { text, .. } => text,
            _ => unreachable!("a hunk row always contains diff content"),
        }
    }
}

impl ExcerptHunk {
    fn read(
        rows: &[DiffRow],
        row_offset: usize,
        selection: &RangeInclusive<usize>,
        old: (u32, u32),
        new: (u32, u32),
    ) -> Option<Self> {
        let mut old_line = old.0 + u32::from(old.1 == 0);
        let mut new_line = new.0 + u32::from(new.1 == 0);
        let mut content = Vec::new();
        for (offset, row) in rows.iter().enumerate() {
            match row {
                DiffRow::Context { .. } => {
                    content.push(HunkRow {
                        row,
                        offset,
                        old_line,
                        new_line,
                        selected: selection.contains(&(row_offset + offset)),
                    });
                    old_line += 1;
                    new_line += 1;
                }
                DiffRow::Delete { .. } => {
                    content.push(HunkRow {
                        row,
                        offset,
                        old_line,
                        new_line,
                        selected: selection.contains(&(row_offset + offset)),
                    });
                    old_line += 1;
                }
                DiffRow::Add { .. } => {
                    content.push(HunkRow {
                        row,
                        offset,
                        old_line,
                        new_line,
                        selected: selection.contains(&(row_offset + offset)),
                    });
                    new_line += 1;
                }
                _ => {}
            }
        }

        let first_selected = content.iter().position(|row| row.selected)?;
        let last_selected = content.iter().rposition(|row| row.selected)?;
        let old_count: u32 = content
            .iter()
            .filter(|row| row.selected)
            .map(HunkRow::old_count)
            .sum();
        let new_count: u32 = content
            .iter()
            .filter(|row| row.selected)
            .map(HunkRow::new_count)
            .sum();
        let context_index = (old_count == 0 || new_count == 0)
            .then(|| {
                content[last_selected + 1..]
                    .iter()
                    .position(HunkRow::is_context)
                    .map(|index| last_selected + 1 + index)
                    .or_else(|| {
                        content[..first_selected]
                            .iter()
                            .rposition(HunkRow::is_context)
                    })
            })
            .flatten();
        let included: Vec<_> = content
            .iter()
            .enumerate()
            .filter(|(index, row)| row.selected || context_index == Some(*index))
            .map(|(_, row)| row)
            .collect();
        let first = included[0];
        let old_count = included.iter().map(|row| row.old_count()).sum();
        let new_count = included.iter().map(|row| row.new_count()).sum();
        let mut lines = Vec::new();
        for row in included {
            lines.push(row.text().to_owned());
            if matches!(
                rows.get(row.offset + 1),
                Some(DiffRow::Meta { text }) if text == "\\ No newline at end of file"
            ) {
                lines.push("\\ No newline at end of file".to_owned());
            }
        }
        Some(Self {
            old_start: first.old_line.saturating_sub(u32::from(old_count == 0)),
            old_count,
            new_start: first.new_line.saturating_sub(u32::from(new_count == 0)),
            new_count,
            lines,
        })
    }
}
