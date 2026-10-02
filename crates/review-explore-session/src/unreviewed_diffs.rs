//! The unreviewed lines as files the agent reads: one numbered diff per
//! changed path, so it sees what is left without intersecting a full diff
//! with a list of ranges, and cites the line numbers it reads.

use std::fmt;
use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};

use eyre::WrapErr;
use review_repository::repository::{ChangedFile, Snapshot};
use review_state::{OpenRow, ReviewTracker, RowChange};
use tempfile::TempDir;

/// The unreviewed diffs of one prompt, in a temporary directory of their own
/// that goes away with them. Several reviewers run at once, each prompt of
/// each gets a new directory, and only this user can read it: the diffs are
/// source code.
#[derive(Debug)]
pub(crate) struct UnreviewedDiffs {
    directory: TempDir,
    /// The diffs that could not sit at their source path, as (source path,
    /// file name) pairs.
    displaced: Vec<(String, String)>,
}

/// Lists the displaced diffs, at the top of the directory.
const INDEX: &str = "__herdr_reviewer_index__";
/// What the reviewer's own files at the top of the directory start with.
const RESERVED: &str = "__herdr_reviewer_";

/// One path's unreviewed hunks: each row with its old (base) and new
/// (current) line number, as review marks and citations name them.
struct FileDiff<'a> {
    name: &'a str,
    hunks: &'a [Vec<OpenRow>],
}

impl UnreviewedDiffs {
    /// Write the diffs of `files`, the paths that still have unreviewed lines.
    pub(crate) fn write<'a>(
        tracker: &ReviewTracker,
        snapshot: &Snapshot,
        files: impl Iterator<Item = &'a ChangedFile>,
    ) -> eyre::Result<Self> {
        let directory = tempfile::Builder::new()
            .prefix("herdr-review-unreviewed-")
            .permissions(Permissions::from_mode(0o700))
            .tempdir()?;
        let mut diffs = Self {
            directory,
            displaced: Vec::new(),
        };
        for file in files {
            let name = file.review_path().display();
            let hunks = tracker
                .open_rows(snapshot, file)
                .wrap_err_with(|| name.clone())?;
            let diff = FileDiff {
                name: &name,
                hunks: &hunks,
            };
            diffs
                .place(&name, &diff.to_string())
                .wrap_err_with(|| name.clone())?;
        }
        diffs.write_index()?;
        Ok(diffs)
    }

    pub(crate) fn directory(&self) -> &Path {
        self.directory.path()
    }

    /// The file listing the displaced diffs, when there are any.
    pub(crate) fn index(&self) -> Option<PathBuf> {
        (!self.displaced.is_empty()).then(|| self.directory().join(INDEX))
    }

    /// Write one diff at its source path. Two kinds of path cannot hold it:
    /// one another diff already uses as a file or as a directory (a file
    /// replaced by a directory of the same name, or the reverse), and one
    /// named like the reviewer's own files. The diff then goes to the top of
    /// the directory, and the index says where. Any other failure is an
    /// error.
    fn place(&mut self, name: &str, text: &str) -> std::io::Result<()> {
        // A repository path never leaves the directory.
        if !Path::new(name)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
        {
            return Err(std::io::ErrorKind::InvalidInput.into());
        }
        if !name.starts_with(RESERVED) && !self.taken(name) {
            return self.write_at(name, text);
        }
        let displaced = format!("{RESERVED}displaced_{}__", self.displaced.len() + 1);
        std::fs::write(self.directory().join(&displaced), text)?;
        self.displaced.push((name.to_owned(), displaced));
        Ok(())
    }

    /// Whether another diff already uses `name` as a directory, or one of
    /// its parent directories as a file.
    fn taken(&self, name: &str) -> bool {
        let path = self.directory().join(name);
        path.exists()
            || path
                .ancestors()
                .skip(1)
                .take_while(|ancestor| *ancestor != self.directory())
                .any(Path::is_file)
    }

    fn write_at(&self, name: &str, text: &str) -> std::io::Result<()> {
        let path = self.directory().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, text)
    }

    fn write_index(&self) -> std::io::Result<()> {
        let Some(index) = self.index() else {
            return Ok(());
        };
        let mut text = String::from(
            "These diffs are not at their repository path: another diff uses it as a file or as \
             a directory, or its name is the reviewer's own:\n",
        );
        for (name, displaced) in &self.displaced {
            text.push_str(name);
            text.push_str(" -> ");
            text.push_str(displaced);
            text.push('\n');
        }
        std::fs::write(index, text)
    }
}

impl fmt::Display for FileDiff<'_> {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(output, "{}: unreviewed lines", self.name)?;
        if self.hunks.is_empty() {
            return writeln!(
                output,
                "No text lines to show: a binary, mode or whole-file change."
            );
        }
        writeln!(
            output,
            "old = line in the base, new = line in the current file. A - row without a number\n\
             rewrites a reviewed line: mark it with the numbered rows of its change, or the\n\
             whole file when its change has none."
        )?;
        let width = self
            .hunks
            .iter()
            .flatten()
            .filter_map(|row| row.base_line.max(row.current_line))
            .max()
            .unwrap_or(0)
            .to_string()
            .len()
            .max(3);
        let number = |line: Option<u32>| line.map_or_else(String::new, |line| line.to_string());
        for hunk in self.hunks {
            writeln!(output, "\n{:>width$} {:>width$}", "old", "new")?;
            for row in hunk {
                let (marker, old) = match row.change {
                    RowChange::Unchanged => (' ', row.base_line),
                    RowChange::Removed => ('-', row.base_line),
                    RowChange::Added => ('+', None),
                };
                writeln!(
                    output,
                    "{:>width$} {:>width$} {marker} {}",
                    number(old),
                    number(row.current_line),
                    row.text,
                )?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "unreviewed_diffs.tests.rs"]
mod tests;
