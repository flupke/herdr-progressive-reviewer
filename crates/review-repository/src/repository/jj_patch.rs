//! Git-style patch presentation using jj's own hunk algorithm.

use std::io::Write;

use jj_lib::diff_presentation::LineCompareMode;
use jj_lib::diff_presentation::unified::{DiffLineType, GitDiffPart, unified_diff_hunks};
use jj_lib::merge::Diff;

use super::git_path::GitPath;
use super::{ChangeKind, ChangedFile};

pub(super) struct GitPatch<'a> {
    file: &'a ChangedFile,
    before: GitDiffPart,
    after: GitDiffPart,
    context: usize,
}

impl<'a> GitPatch<'a> {
    pub(super) fn new(
        file: &'a ChangedFile,
        before: GitDiffPart,
        after: GitDiffPart,
        context: usize,
    ) -> Self {
        Self {
            file,
            before,
            after,
            context,
        }
    }

    pub(super) fn render(&self) -> Vec<u8> {
        let mut output = Vec::new();
        self.write_headers(&mut output);
        if !self.before.content.is_binary && !self.after.content.is_binary {
            self.write_hunks(&mut output);
        }
        output
    }

    fn write_headers(&self, output: &mut Vec<u8>) {
        let old = GitPath::new(
            "a/",
            self.file
                .old_path
                .as_ref()
                .unwrap_or(self.file.review_path()),
        );
        let new = GitPath::new(
            "b/",
            self.file
                .new_path
                .as_ref()
                .unwrap_or(self.file.review_path()),
        );
        writeln!(output, "diff --git {old} {new}").expect("write to vector");
        match (self.before.mode, self.after.mode) {
            (None, Some(mode)) => writeln!(output, "new file mode {mode}"),
            (Some(mode), None) => writeln!(output, "deleted file mode {mode}"),
            (Some(old), Some(new)) if old != new => {
                writeln!(output, "old mode {old}\nnew mode {new}")
            }
            _ => Ok(()),
        }
        .expect("write to vector");
        if self.file.change == ChangeKind::Renamed {
            let from = GitPath::new(
                "",
                self.file
                    .old_path
                    .as_ref()
                    .unwrap_or(self.file.review_path()),
            );
            let to = GitPath::new(
                "",
                self.file
                    .new_path
                    .as_ref()
                    .unwrap_or(self.file.review_path()),
            );
            writeln!(output, "rename from {from}\nrename to {to}").expect("write to vector");
        }
        writeln!(output, "index {}..{}", self.before.hash, self.after.hash)
            .expect("write to vector");
        if self.before.content.is_binary || self.after.content.is_binary {
            writeln!(output, "Binary files {old} and {new} differ").expect("write to vector");
            return;
        }
        if self.before.mode.is_none() {
            writeln!(output, "--- /dev/null").expect("write to vector");
        } else {
            writeln!(output, "--- {old}").expect("write to vector");
        }
        if self.after.mode.is_none() {
            writeln!(output, "+++ /dev/null").expect("write to vector");
        } else {
            writeln!(output, "+++ {new}").expect("write to vector");
        }
    }

    fn write_hunks(&self, output: &mut Vec<u8>) {
        let contents = Diff::new(
            self.before.content.contents.as_ref(),
            self.after.content.contents.as_ref(),
        );
        for hunk in unified_diff_hunks(contents, self.context, LineCompareMode::Exact) {
            let left = &hunk.left_line_range;
            let right = &hunk.right_line_range;
            writeln!(
                output,
                "@@ -{},{} +{},{} @@",
                left.start + usize::from(!left.is_empty()),
                left.len(),
                right.start + usize::from(!right.is_empty()),
                right.len()
            )
            .expect("write to vector");
            for (kind, tokens) in hunk.lines {
                output.push(match kind {
                    DiffLineType::Context => b' ',
                    DiffLineType::Removed => b'-',
                    DiffLineType::Added => b'+',
                });
                for (_, text) in tokens {
                    output.extend_from_slice(text);
                }
                if output.last() != Some(&b'\n') {
                    output.extend_from_slice(b"\n\\ No newline at end of file\n");
                }
            }
        }
    }
}
