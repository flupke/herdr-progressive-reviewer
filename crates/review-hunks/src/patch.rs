//! Git-style unified diffs: writing one between two versions, and recovering
//! the old version from a diff and the new one.

use crate::text::{CONTEXT, Change, Lines, changes};

/// The Git-style unified diff from `before` to `after` for one path, or no
/// bytes when the versions are equal.
pub fn unified_diff(path: &str, before: &[u8], after: &[u8]) -> Vec<u8> {
    let changes = changes(before, after);
    if changes.is_empty() {
        return Vec::new();
    }
    let mut diff = UnifiedDiff {
        text: format!("diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n").into_bytes(),
        before: Lines::new(before),
        after: Lines::new(after),
    };
    for group in
        changes.chunk_by(|previous, next| next.before.start - previous.before.end <= 2 * CONTEXT)
    {
        diff.hunk(group);
    }
    diff.text
}

/// The old version of a file, rebuilt from its Git-style unified diff and its
/// new version, or `None` when the diff does not describe `after`.
pub fn reverse_apply(diff: &[u8], after: &[u8]) -> Option<Vec<u8>> {
    let mut patch = ReversePatch {
        after: Lines::new(after),
        before: Vec::new(),
        position: 0,
        removed_last: false,
    };
    let mut in_hunk = false;
    for line in diff.split(|byte| *byte == b'\n') {
        if let Some(new_start) = hunk_new_start(line) {
            patch.copy_until(new_start)?;
            in_hunk = true;
        } else if in_hunk {
            patch.push(line)?;
        }
    }
    patch.copy_until(patch.after.len())?;
    Some(patch.before)
}

struct UnifiedDiff<'a> {
    text: Vec<u8>,
    before: Lines<'a>,
    after: Lines<'a>,
}

impl UnifiedDiff<'_> {
    fn hunk(&mut self, group: &[Change]) {
        let (Some(first), Some(last)) = (group.first(), group.last()) else {
            return;
        };
        let start = first.before.start.saturating_sub(CONTEXT);
        let end = (last.before.end + CONTEXT).min(self.before.len());
        let after_start = first.after.start - (first.before.start - start);
        let after_end = last.after.end + (end - last.before.end);
        self.text.extend_from_slice(
            format!(
                "@@ -{} +{} @@\n",
                header_range(start, end - start),
                header_range(after_start, after_end - after_start)
            )
            .as_bytes(),
        );
        let mut position = start;
        for change in group {
            self.lines(b' ', true, position..change.before.start);
            self.lines(b'-', true, change.before.clone());
            self.lines(b'+', false, change.after.clone());
            position = change.before.end;
        }
        self.lines(b' ', true, position..end);
    }

    fn lines(&mut self, prefix: u8, from_before: bool, range: std::ops::Range<u32>) {
        let side = if from_before {
            &self.before
        } else {
            &self.after
        };
        for line in side.get(range).unwrap_or_default() {
            self.text.push(prefix);
            self.text.extend_from_slice(line);
            if !line.ends_with(b"\n") {
                self.text
                    .extend_from_slice(b"\n\\ No newline at end of file\n");
            }
        }
    }
}

/// A hunk header range: Git names the line before an empty range.
fn header_range(start: u32, count: u32) -> String {
    match count {
        0 => format!("{start},0"),
        1 => format!("{}", start + 1),
        _ => format!("{},{count}", start + 1),
    }
}

/// The zero-based first new-side line of a hunk header.
fn hunk_new_start(line: &[u8]) -> Option<u32> {
    let line = std::str::from_utf8(line.strip_prefix(b"@@ -")?).ok()?;
    let (_, rest) = line.split_once(" +")?;
    let (range, _) = rest.split_once(" @@")?;
    let (start, count) = match range.split_once(',') {
        Some((start, count)) => (start.parse::<u32>().ok()?, count.parse::<u32>().ok()?),
        None => (range.parse::<u32>().ok()?, 1),
    };
    Some(if count == 0 {
        start
    } else {
        start.checked_sub(1)?
    })
}

struct ReversePatch<'a> {
    after: Lines<'a>,
    before: Vec<u8>,
    position: u32,
    removed_last: bool,
}

impl ReversePatch<'_> {
    fn copy_until(&mut self, line: u32) -> Option<()> {
        for kept in self.after.get(self.position..line)? {
            self.before.extend_from_slice(kept);
        }
        self.position = line;
        Some(())
    }

    fn push(&mut self, line: &[u8]) -> Option<()> {
        let Some((&marker, text)) = line.split_first() else {
            return Some(());
        };
        match marker {
            b' ' | b'+' => {
                let current = self.after.line(self.position)?;
                if current.strip_suffix(b"\n").unwrap_or(current) != text {
                    return None;
                }
                if marker == b' ' {
                    self.before.extend_from_slice(current);
                }
                self.position += 1;
                self.removed_last = false;
            }
            b'-' => {
                self.before.extend_from_slice(text);
                self.before.push(b'\n');
                self.removed_last = true;
            }
            b'\\' if self.removed_last => {
                self.before.pop();
            }
            b'\\' => {}
            _ => return None,
        }
        Some(())
    }
}

#[cfg(test)]
#[path = "patch.tests.rs"]
mod tests;
