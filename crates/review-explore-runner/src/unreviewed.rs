//! The unreviewed lines a wakeup lists: what no review mark covers yet.

use std::collections::BTreeSet;
use std::fmt;

use review_explore::SourceSide;
use review_repository::repository::RepoPath;
use review_source::SourceLineRange;

/// The changed lines of a checkpoint that no review mark covers, whoever
/// marked the rest: the reviewer in Files, Jev or an earlier answer.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Unreviewed {
    /// Every changed path of the checkpoint, to tell wholly open directories.
    pub paths: BTreeSet<RepoPath>,
    /// The files with unreviewed lines, in any order.
    pub files: Vec<UnreviewedFile>,
    /// What Jev did before the round, when it ran.
    pub jev: Option<String>,
    pub status: UnreviewedStatus,
}

/// Whether the lines could be listed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UnreviewedStatus {
    /// The lines are listed, with what the agent should know about them.
    Listed { notice: Option<String> },
    /// The lines could not be read, and why.
    Unavailable(String),
}

impl Default for UnreviewedStatus {
    fn default() -> Self {
        Self::Listed { notice: None }
    }
}

/// One file's unreviewed lines.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnreviewedFile {
    pub path: RepoPath,
    pub lines: UnreviewedLines,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UnreviewedLines {
    /// Nothing in the file is reviewed; `changed` counts its changed lines.
    Whole { changed: u64 },
    /// Some lines are reviewed; these are not.
    Ranges(Vec<UnreviewedRange>),
}

/// Lines on one side: the base's for old, the current file's for new.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnreviewedRange {
    pub side: SourceSide,
    pub lines: SourceLineRange,
    /// The lines rewrite what was reviewed before.
    pub since_review: bool,
}

/// One listed line of the block: a wholly open directory or a file.
struct Entry {
    path: String,
    text: String,
}

impl UnreviewedLines {
    fn count(&self) -> u64 {
        match self {
            Self::Whole { changed } => *changed,
            Self::Ranges(ranges) => ranges
                .iter()
                .map(|range| u64::from(range.lines.count()))
                .sum(),
        }
    }
}

impl UnreviewedFile {
    fn whole(&self) -> bool {
        matches!(self.lines, UnreviewedLines::Whole { .. })
    }

    fn describe(&self) -> String {
        match &self.lines {
            UnreviewedLines::Whole { changed: 0 } => "whole file".into(),
            UnreviewedLines::Whole { changed } => format!("whole file · {changed} lines"),
            UnreviewedLines::Ranges(ranges) => ranges
                .iter()
                .map(|range| {
                    let since = if range.since_review {
                        " (since review)"
                    } else {
                        ""
                    };
                    format!("{} {}{since}", range.side, range.lines)
                })
                .collect::<Vec<_>>()
                .join(", "),
        }
    }
}

impl Unreviewed {
    /// The outermost directory holding `path` whose every changed file is
    /// wholly open, when it holds at least two.
    fn open_directory(&self, path: &RepoPath, whole: &BTreeSet<&RepoPath>) -> Option<String> {
        let mut directory = String::new();
        for part in path
            .display()
            .split('/')
            .collect::<Vec<_>>()
            .split_last()?
            .1
        {
            directory.push_str(part);
            directory.push('/');
            let mut inside = self
                .paths
                .iter()
                .filter(|path| path.display().starts_with(directory.as_str()));
            let all_open = inside.clone().all(|path| whole.contains(path));
            if all_open && inside.nth(1).is_some() {
                return Some(directory);
            }
        }
        None
    }

    fn entries(&self) -> Vec<Entry> {
        let whole = self
            .files
            .iter()
            .filter(|file| file.whole())
            .map(|file| &file.path)
            .collect::<BTreeSet<_>>();
        let mut entries: Vec<Entry> = Vec::new();
        let mut directories = BTreeSet::new();
        for file in &self.files {
            // Only a wholly open file has a wholly open directory.
            match self.open_directory(&file.path, &whole) {
                Some(directory) => {
                    if directories.insert(directory.clone()) {
                        let files = self.files_in(&directory);
                        let lines = files.iter().map(|file| file.lines.count()).sum::<u64>();
                        entries.push(Entry {
                            text: format!("all {} files · {lines} lines", files.len()),
                            path: directory,
                        });
                    }
                }
                None => entries.push(Entry {
                    path: file.path.display(),
                    text: file.describe(),
                }),
            }
        }
        entries.sort_by(|left, right| left.path.cmp(&right.path));
        entries
    }

    fn files_in(&self, directory: &str) -> Vec<&UnreviewedFile> {
        self.files
            .iter()
            .filter(|file| file.path.display().starts_with(directory))
            .collect()
    }
}

impl fmt::Display for Unreviewed {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(jev) = &self.jev {
            writeln!(output, "\nJev: {jev}")?;
        }
        match &self.status {
            UnreviewedStatus::Unavailable(why) => {
                return writeln!(output, "\nUnreviewed lines: unavailable: {why}");
            }
            UnreviewedStatus::Listed {
                notice: Some(notice),
            } => writeln!(output, "\nNote: {notice}")?,
            UnreviewedStatus::Listed { notice: None } => {}
        }
        if self.files.is_empty() {
            return writeln!(
                output,
                "\nUnreviewed lines: none; every changed line is reviewed."
            );
        }
        let lines = self
            .files
            .iter()
            .map(|file| file.lines.count())
            .sum::<u64>();
        writeln!(
            output,
            "\nUnreviewed lines: {lines} changed lines in {} files (old = base lines, new = current lines)",
            self.files.len()
        )?;
        let entries = self.entries();
        let width = entries
            .iter()
            .map(|entry| entry.path.chars().count())
            .max()
            .unwrap_or(0);
        for entry in entries {
            writeln!(output, "  {:width$}  {}", entry.path, entry.text)?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "unreviewed.tests.rs"]
mod tests;
