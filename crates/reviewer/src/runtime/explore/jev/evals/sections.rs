use review_repository::diff::DiffRow;

use super::dataset::coordinate;

/// Align matching Rust functions inside a coarse replacement before splitting.
/// Unrecognized or unmatched sections remain paired as one edit.
pub(super) struct Sections<'a> {
    functions: Vec<&'a [DiffRow]>,
}

impl<'a> Sections<'a> {
    pub(super) fn align(rows: &'a [DiffRow]) -> Vec<DiffRow> {
        let mut aligned = Vec::new();
        for run in rows.chunk_by(|a, b| coordinate(a).is_some() == coordinate(b).is_some()) {
            let middle = run
                .iter()
                .take_while(|row| matches!(row, DiffRow::Delete { .. }))
                .count();
            let (old, new) = run.split_at(middle);
            match (Self::functions(old), Self::functions(new)) {
                (Some(old), Some(new)) if old.matches(&new) => {
                    for (before, after) in old.functions.into_iter().zip(new.functions) {
                        aligned.extend_from_slice(before);
                        aligned.extend_from_slice(after);
                    }
                }
                _ => aligned.extend_from_slice(run),
            }
        }
        aligned
    }

    fn matches(&self, new: &Self) -> bool {
        self.functions.len() > 1
            && self.functions.len() == new.functions.len()
            && self
                .functions
                .iter()
                .zip(&new.functions)
                .all(|(old, new)| Self::identity(&old[0]) == Self::identity(&new[0]))
    }

    fn functions(rows: &'a [DiffRow]) -> Option<Self> {
        let mut remaining = rows;
        let mut sections = Vec::new();
        while !remaining.is_empty() {
            let (_, indent) = Self::identity(&remaining[0])?;
            let end = remaining.iter().position(|row| {
                let text = Self::text(row);
                text.trim() == "}" && text.len() - text.trim_start().len() == indent
            })? + 1;
            let trailing = remaining[end..]
                .iter()
                .take_while(|row| Self::text(row).trim().is_empty())
                .count();
            let (section, rest) = remaining.split_at(end + trailing);
            sections.push(section);
            remaining = rest;
        }
        Some(Self {
            functions: sections,
        })
    }

    fn identity(row: &DiffRow) -> Option<(&str, usize)> {
        let text = Self::text(row);
        let indent = text.len() - text.trim_start().len();
        let header = text.trim_start();
        let function = header
            .strip_prefix("fn ")
            .or_else(|| header.strip_prefix("pub fn "))
            .or_else(|| header.strip_prefix("pub(crate) fn "))?;
        let name = function.split(['(', '<', ' ']).next()?;
        (!name.is_empty()).then_some((name, indent))
    }

    fn text(row: &DiffRow) -> &str {
        match row {
            // Parsed diff rows retain their single-character +/- prefix.
            DiffRow::Add { text, .. } | DiffRow::Delete { text, .. } => &text[1..],
            _ => "",
        }
    }
}
