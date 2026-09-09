use review_repository::{
    diff::{DiffRow, parse_file_diff},
    repository::ChangedFile,
};
use review_threads::{ReviewThread, ThreadId};
use syntax_highlighting::{HighlightedRow, SyntaxHighlighter};

pub(super) struct OriginalCode {
    pub(super) thread: ThreadId,
    pub(super) rows: Vec<HighlightedRow>,
    pub(super) number_width: usize,
}

impl OriginalCode {
    fn source_rows(thread: &ReviewThread) -> Vec<DiffRow> {
        let parsed = parse_file_diff(
            thread.excerpt.as_bytes(),
            &ChangedFile::modified(thread.path()),
        );
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
        let old = thread
            .anchor
            .old_lines
            .as_ref()
            .map_or(0, |range| range.start);
        let new = thread
            .anchor
            .new_lines
            .as_ref()
            .map_or(old, |range| range.start);
        thread
            .excerpt
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

    pub(super) fn new(thread: &ReviewThread, highlighter: &SyntaxHighlighter) -> Self {
        let mut rows = highlighter
            .highlight(
                thread.path(),
                Self::source_rows(thread),
                thread.anchor.old_content.as_deref(),
                thread.anchor.new_content.as_deref(),
            )
            .rows;
        // Preserve the saved excerpt even when an old history lacks full source content.
        let text = rows
            .iter()
            .map(|row| Self::text(&row.diff))
            .collect::<Vec<_>>()
            .join("\n");
        let fallback = highlighter.highlight_snippet(thread.path(), &text);
        for (row, fallback) in rows.iter_mut().zip(fallback) {
            let source_missing = match row.diff {
                DiffRow::Delete { .. } => thread.anchor.old_content.is_none(),
                _ => thread.anchor.new_content.is_none(),
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
            thread: thread.id.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use review_guide::{DiffRangeAnchor, GuideAnchorKind};
    use review_threads::{Post, ReviewThreads};
    use ui_theme::Theme;

    #[test]
    fn saved_diff_keeps_line_numbers_changes_and_syntax_with_or_without_full_source() {
        let theme = Theme::default();
        let highlighter = SyntaxHighlighter::new(theme.syntax, theme.palette.text);
        for full_source in [true, false] {
            let post = Post::start(
                DiffRangeAnchor {
                    source_checkpoint: "original".into(),
                    old_path: Some("src/lib.rs".into()),
                    new_path: Some("src/lib.rs".into()),
                    old_lines: Some(0..1),
                    new_lines: Some(0..1),
                    target_kind: GuideAnchorKind::Lines,
                    source_hunk_count: 1,
                    old_content: full_source.then(|| b"fn old() {}\n".to_vec()),
                    new_content: full_source.then(|| b"fn new() {}\n".to_vec()),
                    diff_hash: String::new(),
                },
                "@@ -1 +1 @@\n-fn old() {}\n+fn new() {}".into(),
                "Why change it?".into(),
            );
            let mut book = ReviewThreads::new("review".into());
            book.post(post).unwrap();
            let code = OriginalCode::new(&book.threads()[0], &highlighter);
            assert!(matches!(
                code.rows[0].diff,
                DiffRow::Delete { old_line: 1, .. }
            ));
            assert!(matches!(
                code.rows[1].diff,
                DiffRow::Add { new_line: 1, .. }
            ));
            assert_eq!(OriginalCode::text(&code.rows[0].diff), "fn old() {}");
            assert_eq!(OriginalCode::text(&code.rows[1].diff), "fn new() {}");
            for row in &code.rows {
                assert!(
                    row.tokens
                        .iter()
                        .any(|token| token.color != theme.palette.text)
                );
            }
        }
    }
}
