//! Request state formats and compact unified-diff encoding.

#[cfg(test)]
use super::hunk_starts;
use review_repository::diff::DiffRow;
use serde_json::{Value, json};
use std::{fmt::Write, ops::Range};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(all(test, feature = "jev-evals"), derive(serde::Deserialize))]
#[cfg_attr(all(test, feature = "jev-evals"), serde(rename_all = "snake_case"))]
pub(in crate::runtime::explore::jev) enum Format {
    RowsLegacy,
    RowsNoOmissions,
    UnifiedCompact,
}

impl Default for Format {
    fn default() -> Self {
        Self::RowsLegacy
    }
}

pub(in crate::runtime::explore::jev) fn row_json(row: &DiffRow, target: bool) -> Value {
    match row {
        DiffRow::Add { new_line, text } => {
            json!({"kind":"added","new":new_line,"text":text,"target":target})
        }
        DiffRow::Delete { old_line, text } => {
            json!({"kind":"deleted","old":old_line,"text":text,"target":target})
        }
        DiffRow::Context {
            old_line,
            new_line,
            text,
        } => json!({"kind":"unchanged","old":old_line,"new":new_line,"text":text,"target":false}),
        _ => unreachable!("hunk content only"),
    }
}

pub(super) struct WindowInput<'a> {
    pub(super) path: &'a str,
    pub(super) language: &'a str,
    pub(super) file_context: &'a Value,
    pub(super) rows: &'a [DiffRow],
    pub(super) target: Range<usize>,
    pub(super) context: Range<usize>,
    pub(super) start: Option<(u32, u32)>,
}

impl WindowInput<'_> {
    pub(super) fn state(&self, format: Format) -> Value {
        if format == Format::UnifiedCompact {
            let (diff, target_rows, changed_rows) = self.unified_diff();
            let mut state = json!({"path":self.path, "language_hint":self.language,
                "file_context":self.file_context, "diff":diff});
            if target_rows.len() != changed_rows {
                state["target_rows"] = json!(target_rows);
            }
            return state;
        }
        let mut state = json!({
            "path":self.path, "language_hint":self.language, "file_context":self.file_context,
            "rows": self.rows[self.context.clone()].iter().enumerate().map(|(i, row)|
                row_json(row, self.target.contains(&(i + self.context.start)))).collect::<Vec<_>>(),
        });
        if format == Format::RowsLegacy {
            state["omissions"] = json!([
                "Other hunks, callers and helpers are not supplied; rows outside this window of the original hunk are omitted."
            ]);
        }
        state
    }

    fn unified_diff(&self) -> (String, Vec<usize>, usize) {
        let mut writer = DiffWriter::new(self.rows, self.start);
        for (index, row) in self.rows.iter().enumerate().take(self.context.end) {
            writer.push(
                row,
                index >= self.context.start,
                self.target.contains(&index),
            );
        }
        writer.finish()
    }
}

struct DiffContent<'a> {
    old: Option<u32>,
    new: Option<u32>,
    text: &'a str,
}

impl<'a> DiffContent<'a> {
    fn from_row(row: &'a DiffRow) -> Self {
        match row {
            DiffRow::Add { new_line, text } => Self {
                old: None,
                new: Some(*new_line),
                text,
            },
            DiffRow::Delete { old_line, text } => Self {
                old: Some(*old_line),
                new: None,
                text,
            },
            DiffRow::Context {
                old_line,
                new_line,
                text,
            } => Self {
                old: Some(*old_line),
                new: Some(*new_line),
                text,
            },
            _ => unreachable!("hunk content only"),
        }
    }

    fn changed(&self) -> bool {
        self.old.is_none() || self.new.is_none()
    }
}

struct DiffWriter {
    old: u32,
    new: u32,
    segment_old: u32,
    segment_new: u32,
    old_count: u32,
    new_count: u32,
    segment: String,
    diff: String,
    content_row: usize,
    changed_rows: usize,
    target_rows: Vec<usize>,
}

impl DiffWriter {
    fn new(rows: &[DiffRow], start: Option<(u32, u32)>) -> Self {
        let (old, new) = start.unwrap_or_else(|| {
            let old = rows
                .iter()
                .find_map(|row| DiffContent::from_row(row).old)
                .unwrap_or(0);
            let new = rows
                .iter()
                .find_map(|row| DiffContent::from_row(row).new)
                .unwrap_or(0);
            (old, new)
        });
        Self {
            old,
            new,
            segment_old: old,
            segment_new: new,
            old_count: 0,
            new_count: 0,
            segment: String::new(),
            diff: String::new(),
            content_row: 0,
            changed_rows: 0,
            target_rows: Vec::new(),
        }
    }

    fn push(&mut self, row: &DiffRow, visible: bool, target: bool) {
        let content = DiffContent::from_row(row);
        let gap = content.old.is_some_and(|line| line != self.old)
            || content.new.is_some_and(|line| line != self.new);
        if gap {
            self.flush();
        }
        if let Some(line) = content.old {
            self.old = line;
        }
        if let Some(line) = content.new {
            self.new = line;
        }
        if self.segment.is_empty() {
            self.segment_old = self.old;
            self.segment_new = self.new;
        }
        if visible {
            self.include(&content, target);
        }
        if content.old.is_some() {
            self.old = self.old.saturating_add(1);
        }
        if content.new.is_some() {
            self.new = self.new.saturating_add(1);
        }
    }

    fn include(&mut self, content: &DiffContent<'_>, target: bool) {
        self.segment.push_str(content.text);
        self.segment.push('\n');
        self.content_row += 1;
        if content.changed() {
            self.changed_rows += 1;
            if target {
                self.target_rows.push(self.content_row);
            }
        }
        if content.old.is_some() {
            self.old_count += 1;
        }
        if content.new.is_some() {
            self.new_count += 1;
        }
    }

    fn flush(&mut self) {
        if self.segment.is_empty() {
            return;
        }
        append_segment(
            &mut self.diff,
            &self.segment,
            self.segment_old,
            self.old_count,
            self.segment_new,
            self.new_count,
        );
        self.segment.clear();
        self.old_count = 0;
        self.new_count = 0;
    }

    fn finish(mut self) -> (String, Vec<usize>, usize) {
        self.flush();
        (self.diff, self.target_rows, self.changed_rows)
    }
}

fn append_segment(
    diff: &mut String,
    content: &str,
    old: u32,
    old_count: u32,
    new: u32,
    new_count: u32,
) {
    let old_start = if old_count == 0 {
        old.saturating_sub(1)
    } else {
        old
    };
    let new_start = if new_count == 0 {
        new.saturating_sub(1)
    } else {
        new
    };
    writeln!(
        diff,
        "@@ -{old_start},{old_count} +{new_start},{new_count} @@"
    )
    .expect("write to String");
    diff.push_str(content);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_hunk_uses_original_zero_count_anchor() {
        let cases = [
            (
                DiffRow::Hunk {
                    old_start: 10,
                    old_count: 0,
                    new_start: 11,
                    new_count: 1,
                },
                DiffRow::Add {
                    new_line: 11,
                    text: "+added".into(),
                },
                "@@ -10,0 +11,1 @@\n+added\n",
            ),
            (
                DiffRow::Hunk {
                    old_start: 10,
                    old_count: 1,
                    new_start: 10,
                    new_count: 0,
                },
                DiffRow::Delete {
                    old_line: 10,
                    text: "-removed".into(),
                },
                "@@ -10,1 +10,0 @@\n-removed\n",
            ),
            (
                DiffRow::Hunk {
                    old_start: 0,
                    old_count: 0,
                    new_start: 1,
                    new_count: 1,
                },
                DiffRow::Add {
                    new_line: 1,
                    text: "+new file".into(),
                },
                "@@ -0,0 +1,1 @@\n+new file\n",
            ),
        ];
        for (header, row, expected) in cases {
            let start = hunk_starts(&[header])[0];
            let rows = [row];
            let input = WindowInput {
                path: "file.rs",
                language: "Rust",
                file_context: &Value::Null,
                rows: &rows,
                target: 0..1,
                context: 0..1,
                start: Some(start),
            };
            assert_eq!(input.state(Format::UnifiedCompact)["diff"], expected);
        }
    }
}
