//! What a prompt says about the unreviewed lines: where their diffs are.

use std::fmt;
use std::path::PathBuf;

/// The changed lines of a checkpoint that no review mark covers, as a
/// prompt gives them: the directory holding their numbered diffs, each
/// named like its source file.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Unreviewed {
    pub directory: PathBuf,
    /// How many files still have unreviewed lines.
    pub files: usize,
    /// The index file naming the diffs that could not sit at their source
    /// path, when there are any.
    pub index: Option<PathBuf>,
    /// What the agent should know about the lines.
    pub notice: Option<String>,
}

impl fmt::Display for Unreviewed {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(notice) = &self.notice {
            writeln!(output, "\nNote: {notice}")?;
        }
        if self.files == 0 {
            return writeln!(
                output,
                "\nUnreviewed diffs: none; every changed line is reviewed."
            );
        }
        writeln!(
            output,
            "\nUnreviewed diffs: {}/<repository path>, for the {} {} with unreviewed lines",
            self.directory.display(),
            self.files,
            if self.files == 1 { "file" } else { "files" }
        )?;
        if let Some(index) = &self.index {
            writeln!(
                output,
                "Displaced diffs: {} says where the diffs that are not at their repository path are",
                index.display()
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "unreviewed.tests.rs"]
mod tests;
