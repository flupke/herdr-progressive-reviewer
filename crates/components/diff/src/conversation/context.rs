use review_repository::{
    diff::{DiffRow, parse_file_diff},
    repository::ChangedFile,
};
use review_threads::{ThreadId, ThreadSource};
use syntax_highlighting::{HighlightedRow, SyntaxHighlighter};

pub(super) struct OriginalCode {
    pub(super) thread: ThreadId,
    pub(super) rows: Vec<HighlightedRow>,
    pub(super) number_width: usize,
}

impl OriginalCode {
    fn source_rows(path: &str, code: &ThreadSource) -> Vec<DiffRow> {
        let parsed = parse_file_diff(code.excerpt.as_bytes(), &ChangedFile::modified(path));
        let rows: Vec<_> = parsed
            .into_iter()
            .filter(|row| {
                matches!(
                    row,
                    DiffRow::Add { .. } | DiffRow::Delete { .. } | DiffRow::Context { .. }
                )
            })
            .collect();
        if !rows.is_empty() {
            return rows;
        }
        let old = code
            .anchor
            .old_lines
            .as_ref()
            .map_or(0, |range| range.start);
        let new = code
            .anchor
            .new_lines
            .as_ref()
            .map_or(old, |range| range.start);
        code.excerpt
            .lines()
            .enumerate()
            .map(|(index, text)| {
                let offset = u32::try_from(index).unwrap_or(u32::MAX).saturating_add(1);
                DiffRow::Context {
                    old_line: old.saturating_add(offset),
                    new_line: new.saturating_add(offset),
                    text: format!(" {text}"),
                }
            })
            .collect()
    }

    /// The saved code of thread `thread`, a selection of `path`.
    pub(super) fn new(
        thread: ThreadId,
        path: &str,
        code: &ThreadSource,
        highlighter: &SyntaxHighlighter,
    ) -> Self {
        let mut rows = highlighter
            .highlight(
                path,
                Self::source_rows(path, code),
                code.anchor.old_content.as_deref(),
                code.anchor.new_content.as_deref(),
            )
            .rows;
        // Preserve the saved excerpt even when an old history lacks full source content.
        let text = rows
            .iter()
            .map(|row| Self::text(&row.diff))
            .collect::<Vec<_>>()
            .join("\n");
        let fallback = highlighter.highlight_snippet(path, &text);
        for (row, fallback) in rows.iter_mut().zip(fallback) {
            let source_missing = match row.diff {
                DiffRow::Delete { .. } => code.anchor.old_content.is_none(),
                _ => code.anchor.new_content.is_none(),
            };
            if source_missing
                || row
                    .tokens
                    .iter()
                    .map(|token| token.text.as_str())
                    .collect::<String>()
                    != Self::text(&row.diff)
            {
                row.tokens = fallback;
            }
        }
        let last = rows
            .iter()
            .map(|row| match row.diff {
                DiffRow::Delete { old_line, .. } => old_line,
                DiffRow::Context { new_line, .. } | DiffRow::Add { new_line, .. } => new_line,
                _ => 0,
            })
            .max()
            .unwrap_or_default();
        Self {
            thread,
            rows,
            number_width: last.to_string().len(),
        }
    }

    fn text(row: &DiffRow) -> &str {
        match row {
            DiffRow::Context { text, .. }
            | DiffRow::Add { text, .. }
            | DiffRow::Delete { text, .. } => text.get(1..).unwrap_or_default(),
            _ => "",
        }
    }
}
