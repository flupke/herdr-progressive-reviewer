//! The changed lines that agent locations name in one file, as line marks select them.

use review_hunks::LineSelection;
use review_repository::repository::ChangedFile;

use crate::{CodeLocation, SourceSide};

/// The changed lines some locations of one file name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NamedLines {
    /// A location names the whole file.
    Whole,
    /// Zero-based lines on each side: base lines on the old side, current lines on the new.
    Lines(LineSelection),
}

impl NamedLines {
    /// The lines `locations`, all in one file, name.
    pub fn of<'a>(locations: impl IntoIterator<Item = &'a CodeLocation>) -> Self {
        let mut selection = LineSelection::default();
        for location in locations {
            let Some(lines) = &location.lines else {
                return Self::Whole;
            };
            let lines = lines.first_line.saturating_sub(1)..lines.last_line;
            match location.side {
                SourceSide::Old => selection.removed.extend(lines),
                SourceSide::New => selection.added.extend(lines),
            }
        }
        Self::Lines(selection)
    }

    /// The lines those of `locations` that are in `file` name; `None` when none is.
    pub fn in_file<'a>(
        file: &ChangedFile,
        locations: impl IntoIterator<Item = &'a CodeLocation>,
    ) -> Option<Self> {
        let mut named = locations
            .into_iter()
            .filter(|location| location.names(file))
            .peekable();
        named.peek()?;
        Some(Self::of(named))
    }

    /// The named lines, with `whole` giving every line of the file.
    pub fn or_whole(self, whole: impl FnOnce() -> LineSelection) -> LineSelection {
        match self {
            Self::Whole => whole(),
            Self::Lines(selection) => selection,
        }
    }

    /// Whether the zero-based `line` on `side` is named.
    pub fn contains(&self, side: SourceSide, line: u32) -> bool {
        match self {
            Self::Whole => true,
            Self::Lines(selection) => match side {
                SourceSide::Old => selection.removed.contains(&line),
                SourceSide::New => selection.added.contains(&line),
            },
        }
    }
}

#[cfg(test)]
#[path = "named_lines.tests.rs"]
mod tests;
