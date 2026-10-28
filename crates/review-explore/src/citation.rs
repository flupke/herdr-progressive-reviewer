//! The lines a citation puts before the reviewer, as the pane's evidence viewer and the
//! Explore page find them in the change.

use std::borrow::Cow;
use std::path::Path;

use review_repository::diff::{DiffRow, NoticeKind, parse_file_diff};
use review_repository::repository::{RepoPath, Repository, Snapshot};
use review_source::SourceLineRange;

use crate::{CodeLocation, Comparison, Source, SourceSide};

/// The source a citation names, read as text, with its cited range checked.
pub struct CitedSource<'a> {
    pub source: Cow<'a, Source>,
    pub content: String,
    /// The index in [`Comparison::files`] of the changed file whose diff holds the source:
    /// the change touches the cited path on the cited side. `None` for a source the change
    /// leaves alone.
    pub file: Option<usize>,
}

/// Why a citation shows no lines.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Uncitable {
    /// The path leaves the repository.
    Path,
    /// The source cannot be read as text.
    Unreadable(String),
    /// The cited lines are not in the source.
    Range,
    /// The citation names the whole file, not lines of it.
    WholeFile,
    /// The change of the cited file has no text diff: a binary file, a conflict, or rows the
    /// diff parser does not know.
    NoTextDiff(NoticeKind),
    /// The file is neither part of the change nor tracked by the repository, such as an ignored
    /// `.env`: see [`Comparison::tracked_cited_lines`].
    Untracked,
    /// The repository no longer holds the code a saved round cited, and the round's saved diff
    /// does not hold the cited lines: see [`Comparison::saved_cited_lines`].
    Gone,
}

impl std::fmt::Display for Uncitable {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Path => output.write_str("The cited path is outside the repository."),
            Self::Unreadable(error) => write!(
                output,
                "File-level evidence · non-text or unavailable source: {error}"
            ),
            Self::Range => {
                output.write_str("The saved evidence range is unavailable in this source.")
            }
            Self::WholeFile => output.write_str(
                "File-level evidence · the citation names the whole file, not lines of it, \
                 so no lines show here.",
            ),
            Self::NoTextDiff(kind) => output.write_str(no_text_diff(*kind)),
            Self::Untracked => output.write_str(
                "This file is not part of the repository's tracked files, so its lines do not \
                 show here.",
            ),
            Self::Gone => output.write_str(
                "The code this round cited is no longer available: the repository no longer \
                 holds the commit the round reviewed, and the round's saved diff does not hold \
                 these lines.",
            ),
        }
    }
}

/// [`Uncitable::Range`] unless `content` holds the lines `range`.
fn check_range(range: &SourceLineRange, content: &str) -> Result<(), Uncitable> {
    if range.first_line == 0
        || range.last_line < range.first_line
        || range.last_line as usize > content.lines().count()
    {
        return Err(Uncitable::Range);
    }
    Ok(())
}

/// Why a file whose change is `kind` shows no lines.
fn no_text_diff(kind: NoticeKind) -> &'static str {
    match kind {
        NoticeKind::Binary => "File-level evidence · the change of this file is binary.",
        NoticeKind::Conflict => {
            "File-level evidence · this file has an unresolved conflict, so the change shows no \
             lines of it."
        }
        NoticeKind::Unsupported => {
            "File-level evidence · the change of this file has no text diff."
        }
    }
}

/// The cited lines of one side of a file, as diff rows: the rows of the change where it
/// touches them, unchanged rows elsewhere, and the other side's rows between the first cited
/// line and the last.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CitedLines {
    /// The path whose syntax colors the lines.
    pub path: String,
    pub rows: Vec<DiffRow>,
    /// The whole file before the change, when it has one.
    pub old_content: Option<Vec<u8>>,
    /// The whole file after the change, when it has one.
    pub new_content: Option<Vec<u8>>,
}

impl CitedLines {
    /// The lines `location` cites in a file whose change is `diff`, and whose content is
    /// `old_content` before the change and `new_content` after it.
    pub fn in_diff(
        location: &CodeLocation,
        diff: &[DiffRow],
        old_content: Option<Vec<u8>>,
        new_content: Option<Vec<u8>>,
    ) -> Result<Self, Uncitable> {
        let range = location.lines.as_ref().ok_or(Uncitable::WholeFile)?;
        if let Some(DiffRow::Notice { kind, .. }) = diff
            .iter()
            .find(|row| matches!(row, DiffRow::Notice { .. }))
        {
            return Err(Uncitable::NoTextDiff(*kind));
        }
        let content = location
            .side
            .pick(old_content.as_deref(), new_content.as_deref())
            .and_then(|content| std::str::from_utf8(content).ok())
            .ok_or(Uncitable::Range)?;
        let rows = FileRows::new(diff, location.side, content).cited(range)?;
        Ok(Self {
            path: location.path.display(),
            rows,
            old_content,
            new_content,
        })
    }

    /// The lines `range` of the change's rows `diff` of one file, on `side`, with no content:
    /// [`Uncitable::Gone`] unless the rows hold every line of the range.
    fn in_diff_alone(
        path: String,
        side: SourceSide,
        range: &SourceLineRange,
        diff: &[DiffRow],
    ) -> Result<Self, Uncitable> {
        if let Some(DiffRow::Notice { kind, .. }) = diff
            .iter()
            .find(|row| matches!(row, DiffRow::Notice { .. }))
        {
            return Err(Uncitable::NoTextDiff(*kind));
        }
        let file = FileRows {
            side,
            rows: diff
                .iter()
                .filter(|row| {
                    matches!(
                        row,
                        DiffRow::Context { .. } | DiffRow::Add { .. } | DiffRow::Delete { .. }
                    )
                })
                .cloned()
                .collect(),
        };
        let rows = file.cited(range).map_err(|_| Uncitable::Gone)?;
        // Between two hunks, the rows skip the lines the change left alone.
        let lines = rows.iter().filter(|row| file.line(row).is_some()).count();
        if lines != (range.last_line - range.first_line + 1) as usize {
            return Err(Uncitable::Gone);
        }
        Ok(Self {
            path,
            rows,
            old_content: None,
            new_content: None,
        })
    }

    /// The lines `range` of a file the change leaves alone: unchanged rows numbered alike on
    /// both sides.
    fn unchanged(path: String, range: &SourceLineRange, content: String) -> Self {
        let lines: Vec<&str> = content.lines().collect();
        let rows = (range.first_line..=range.last_line)
            .map(|line| DiffRow::Context {
                old_line: line,
                new_line: line,
                text: format!(" {}", lines[line as usize - 1]),
            })
            .collect();
        let content = content.into_bytes();
        Self {
            path,
            rows,
            old_content: Some(content.clone()),
            new_content: Some(content),
        }
    }
}

/// Every row of one side of a changed file: the rows of its diff, with the unchanged lines
/// between its hunks filled in from that side's content.
struct FileRows {
    side: SourceSide,
    rows: Vec<DiffRow>,
}

impl FileRows {
    fn new(diff: &[DiffRow], side: SourceSide, content: &str) -> Self {
        let lines: Vec<&str> = content.lines().collect();
        let mut file = Self {
            side,
            rows: Vec::new(),
        };
        let (mut old, mut new) = (1, 1);
        for row in diff {
            let unchanged_before = match row {
                DiffRow::Context { new_line, .. } | DiffRow::Add { new_line, .. } => {
                    new_line.saturating_sub(new)
                }
                DiffRow::Delete { old_line, .. } => old_line.saturating_sub(old),
                _ => continue,
            };
            file.unchanged(&lines, &mut old, &mut new, unchanged_before);
            match row {
                DiffRow::Context {
                    old_line, new_line, ..
                } => (old, new) = (old_line + 1, new_line + 1),
                DiffRow::Add { new_line, .. } => new = new_line + 1,
                DiffRow::Delete { old_line, .. } => old = old_line + 1,
                _ => {}
            }
            file.rows.push(row.clone());
        }
        let rest = u32::try_from(lines.len())
            .unwrap_or(u32::MAX)
            .saturating_sub(side.pick(old, new) - 1);
        file.unchanged(&lines, &mut old, &mut new, rest);
        file
    }

    /// Appends `count` unchanged rows from `old` and `new` on, with the text of this side's
    /// `lines`.
    fn unchanged(&mut self, lines: &[&str], old: &mut u32, new: &mut u32, count: u32) {
        for _ in 0..count {
            let line = self.side.pick(*old, *new);
            let text = lines.get(line as usize - 1).copied().unwrap_or_default();
            self.rows.push(DiffRow::Context {
                old_line: *old,
                new_line: *new,
                text: format!(" {text}"),
            });
            *old += 1;
            *new += 1;
        }
    }

    /// The line number of `row` on this side, when the side has the row.
    fn line(&self, row: &DiffRow) -> Option<u32> {
        match (self.side, row) {
            (
                SourceSide::Old,
                DiffRow::Context { old_line: line, .. } | DiffRow::Delete { old_line: line, .. },
            )
            | (
                SourceSide::New,
                DiffRow::Context { new_line: line, .. } | DiffRow::Add { new_line: line, .. },
            ) => Some(*line),
            _ => None,
        }
    }

    /// The rows from the first line of `range` on this side to its last.
    fn cited(&self, range: &SourceLineRange) -> Result<Vec<DiffRow>, Uncitable> {
        let first = self
            .rows
            .iter()
            .position(|row| self.line(row) == Some(range.first_line));
        let last = self
            .rows
            .iter()
            .rposition(|row| self.line(row) == Some(range.last_line));
        match (first, last) {
            (Some(first), Some(last)) if first <= last => Ok(self.rows[first..=last].to_vec()),
            _ => Err(Uncitable::Range),
        }
    }
}

impl Comparison {
    /// The source `location` names, read from `root`, with its lines checked against it.
    pub fn cited_source(
        &self,
        location: &CodeLocation,
        root: &Path,
    ) -> Result<CitedSource<'_>, Uncitable> {
        let source = self.source(location).ok_or(Uncitable::Path)?;
        let content = source
            .read_text(root)
            .map_err(|error| Uncitable::Unreadable(error.to_string()))?;
        if let Some(range) = &location.lines {
            check_range(range, &content)?;
        }
        let file = self
            .files
            .iter()
            .position(|file| source.side.path_in(file) == Some(&source.path));
        Ok(CitedSource {
            source,
            content,
            file,
        })
    }

    /// The lines `location` cites, as [`Self::cited_lines`] finds them, when the change touches
    /// the cited file or the repository tracks it; [`Uncitable::Untracked`] for any other file
    /// of the working copy, which a page open from the network must not show.
    pub fn tracked_cited_lines(
        &self,
        location: &CodeLocation,
        root: &Path,
    ) -> Result<CitedLines, Uncitable> {
        if !self.tracks(location, root) {
            return Err(Uncitable::Untracked);
        }
        self.cited_lines(location, root)
    }

    /// Whether the change touches the path `location` names on its side, or the base of the
    /// change has that path. A file the change leaves alone is the same at the base, so the base
    /// has it exactly when the repository tracks it.
    fn tracks(&self, location: &CodeLocation, root: &Path) -> bool {
        let changed = self
            .files
            .iter()
            .any(|file| location.side.path_in(file) == Some(&location.path));
        let base = CodeLocation {
            path: location.path.clone(),
            side: SourceSide::Old,
            lines: None,
        };
        changed
            || self
                .source(&base)
                .is_some_and(|source| source.read(root).is_ok())
    }

    /// The lines `location` cites, read from `root`, in the rows of the change.
    pub fn cited_lines(
        &self,
        location: &CodeLocation,
        root: &Path,
    ) -> Result<CitedLines, Uncitable> {
        let range = location.lines.as_ref().ok_or(Uncitable::WholeFile)?;
        let cited = self.cited_source(location, root)?;
        let path = cited.source.display_path.clone();
        self.lines_of(
            location,
            range,
            path,
            cited.content,
            cited.file,
            |side, path| {
                self.source(&CodeLocation {
                    path: path.clone(),
                    side,
                    lines: None,
                })?
                .read_text(root)
                .ok()
                .map(String::into_bytes)
            },
        )
    }

    /// The lines `location` cites as the round saw them, whatever the working copy holds now:
    /// read from `repository` at the round's checkpoint, else from the change's saved diff;
    /// [`Uncitable::Gone`] when neither holds them.
    pub fn saved_cited_lines(
        &self,
        location: &CodeLocation,
        repository: &Repository,
    ) -> Result<CitedLines, Uncitable> {
        let range = location.lines.as_ref().ok_or(Uncitable::WholeFile)?;
        location.relative_path().ok_or(Uncitable::Path)?;
        let path = location.path.display();
        let index = self
            .files
            .iter()
            .position(|file| location.side.path_in(file) == Some(&location.path));
        let Some(content) = self.saved_file(repository, location.side, &location.path) else {
            let index = index.ok_or(Uncitable::Gone)?;
            return CitedLines::in_diff_alone(path, location.side, range, &self.diff_of(index));
        };
        let text = crate::source::text(content)
            .map_err(|error| Uncitable::Unreadable(error.to_string()))?;
        check_range(range, &text)?;
        self.lines_of(location, range, path, text, index, |side, path| {
            self.saved_file(repository, side, path)
        })
    }

    /// The lines `range` that `location` cites in `content`, the text of its file at `path`:
    /// unchanged rows when the change leaves the file alone, else the rows of the change of its
    /// file `index`, whose other side `read` reads.
    fn lines_of(
        &self,
        location: &CodeLocation,
        range: &SourceLineRange,
        path: String,
        content: String,
        index: Option<usize>,
        read: impl Fn(SourceSide, &RepoPath) -> Option<Vec<u8>>,
    ) -> Result<CitedLines, Uncitable> {
        let Some(index) = index else {
            return Ok(CitedLines::unchanged(path, range, content));
        };
        let file = &self.files[index];
        let content = content.into_bytes();
        let side = |side: SourceSide| {
            if side == location.side {
                return Some(content.clone());
            }
            read(side, side.path_in(file)?)
        };
        CitedLines::in_diff(
            location,
            &self.diff_of(index),
            side(SourceSide::Old),
            side(SourceSide::New),
        )
    }

    /// The rows of the change of its file `index`.
    fn diff_of(&self, index: usize) -> Vec<DiffRow> {
        parse_file_diff(
            self.diffs.get(index).map_or(&[], Vec::as_slice),
            &self.files[index],
        )
    }

    /// The file at `path` on `side` of the change as the round saw it, read from `repository`:
    /// the round's checkpoint for the new side, its base for the old one. `None` when the
    /// repository no longer holds it.
    fn saved_file(
        &self,
        repository: &Repository,
        side: SourceSide,
        path: &RepoPath,
    ) -> Option<Vec<u8>> {
        match side {
            SourceSide::New => repository.file_at(&self.checkpoint.checkpoint, path).ok(),
            SourceSide::Old => {
                let snapshot = Snapshot {
                    identity: self.base.clone()?,
                    files: Vec::new(),
                };
                repository.base_file_at(&snapshot, path).ok()
            }
        }
    }
}

#[cfg(test)]
#[path = "citation.tests.rs"]
mod tests;
