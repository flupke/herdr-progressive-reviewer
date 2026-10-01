//! Line views of one file version and the edits between two versions.

use std::ops::Range;

use gix_imara_diff::{Algorithm, Diff, InternedInput};

/// Unchanged lines shown around each change, as Git shows them.
pub(crate) const CONTEXT: u32 = 3;

/// One file version split into lines that keep their terminators.
pub(crate) struct Lines<'a>(Vec<&'a [u8]>);

/// Lines of the before version replaced by lines of the after version.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Change {
    pub(crate) before: Range<u32>,
    pub(crate) after: Range<u32>,
}

impl<'a> Lines<'a> {
    pub(crate) fn new(text: &'a [u8]) -> Self {
        Self(text.split_inclusive(|byte| *byte == b'\n').collect())
    }

    pub(crate) fn len(&self) -> u32 {
        u32::try_from(self.0.len()).unwrap_or(u32::MAX)
    }

    pub(crate) fn line(&self, index: u32) -> Option<&'a [u8]> {
        self.0.get(usize::try_from(index).ok()?).copied()
    }

    pub(crate) fn get(&self, range: Range<u32>) -> Option<&[&'a [u8]]> {
        self.0
            .get(usize::try_from(range.start).ok()?..usize::try_from(range.end).ok()?)
    }

    /// Lines from `index` to the end.
    pub(crate) fn rest(&self, index: u32) -> Option<&[&'a [u8]]> {
        self.get(index..self.len())
    }
}

/// The line edits that turn `before` into `after`.
pub(crate) fn changes(before: &[u8], after: &[u8]) -> Vec<Change> {
    let input = InternedInput::new(before, after);
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_lines(&input);
    diff.hunks()
        .map(|hunk| Change {
            before: hunk.before,
            after: hunk.after,
        })
        .collect()
}

/// Whether two line ranges share a line. An empty range sits between two
/// lines, so it also meets a range that ends or starts there.
pub(crate) fn overlaps(first: &Range<u32>, second: &Range<u32>) -> bool {
    if first.is_empty() || second.is_empty() {
        first.start <= second.end && second.start <= first.end
    } else {
        first.start < second.end && second.start < first.end
    }
}

/// Where `range` lands on the other side of `edits`, given as (from, to) range
/// pairs in order, or `None` when an edit touches it.
pub(crate) fn translate<'r>(
    edits: impl IntoIterator<Item = (&'r Range<u32>, &'r Range<u32>)>,
    range: &Range<u32>,
) -> Option<Range<u32>> {
    let mut shift = 0_i64;
    for (from, to) in edits {
        if overlaps(from, range) {
            return None;
        }
        if from.end <= range.start {
            shift += i64::from(to.end - to.start) - i64::from(from.end - from.start);
        }
    }
    let start = u32::try_from(i64::from(range.start) + shift).ok()?;
    let end = u32::try_from(i64::from(range.end) + shift).ok()?;
    Some(start..end)
}

/// `target` with `range` replaced by the `replacement` lines of `source`.
pub(crate) fn splice(
    target: &Lines<'_>,
    range: Range<u32>,
    source: &Lines<'_>,
    replacement: Range<u32>,
) -> Option<Vec<u8>> {
    let mut text = Vec::new();
    for line in target
        .get(0..range.start)?
        .iter()
        .chain(source.get(replacement)?)
        .chain(target.rest(range.end)?)
    {
        text.extend_from_slice(line);
    }
    Some(text)
}

#[cfg(test)]
#[path = "text.tests.rs"]
mod tests;
