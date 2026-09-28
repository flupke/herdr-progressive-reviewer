//! Source positions and anchors shared by Explore, diff views, and review threads.

use std::ops::Range;

use gix_imara_diff::{Algorithm, Diff, InternedInput};
use review_types::ReviewUnit;
use serde::{Deserialize, Serialize};

/// The repository identity and exact snapshot used for review.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, schemars::JsonSchema)]
pub struct ReviewCheckpoint {
    pub review_unit: ReviewUnit,
    pub checkpoint: String,
}

impl ReviewCheckpoint {
    pub fn new(review_unit: impl Into<ReviewUnit>, checkpoint: impl Into<String>) -> Self {
        Self {
            review_unit: review_unit.into(),
            checkpoint: checkpoint.into(),
        }
    }

    pub fn matches(&self, review_unit: &ReviewUnit, checkpoint: &str) -> bool {
        &self.review_unit == review_unit && self.checkpoint == checkpoint
    }
}

/// One source location in a displayed diff.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum DiffTarget {
    /// One line range on either or both sides of a text diff.
    Lines {
        path: String,
        old: Option<SourceLineRange>,
        new: Option<SourceLineRange>,
    },
    /// A changed file that has no text hunk.
    File { path: String },
}

impl DiffTarget {
    pub fn path(&self) -> &str {
        match self {
            Self::Lines { path, .. } | Self::File { path } => path,
        }
    }
}

/// One inclusive, one-based line range in a source citation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, schemars::JsonSchema)]
pub struct SourceLineRange {
    pub first_line: u32,
    pub last_line: u32,
}

/// A file frozen at one exact review checkpoint.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FrozenFile {
    pub path: String,
    pub hunk_count: usize,
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    pub old_content: Option<Vec<u8>>,
    pub new_content: Option<Vec<u8>>,
    pub hunks: Vec<FrozenHunk>,
    pub diff_hash: String,
}

/// The two half-open line intervals in one frozen text hunk.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FrozenHunk {
    pub old: Option<Range<u32>>,
    pub new: Option<Range<u32>>,
}

/// An immutable source range used to map a thread through later edits.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DiffRangeAnchor {
    pub source_checkpoint: String,
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    pub old_lines: Option<Range<u32>>,
    pub new_lines: Option<Range<u32>>,
    #[serde(default)]
    pub target_kind: AnchorKind,
    pub source_hunk_count: usize,
    pub old_content: Option<Vec<u8>>,
    pub new_content: Option<Vec<u8>>,
    pub diff_hash: String,
}

impl DiffRangeAnchor {
    /// Map only the new-side interval into a current working-tree file.
    pub fn map_new_lines(&self, content: &[u8]) -> Option<Range<u32>> {
        map_side(
            self.new_lines.clone(),
            self.new_content.as_deref(),
            Some(content),
        )
        .ok()
        .flatten()
    }

    /// Map both source intervals through edits, retaining only unchanged anchored code.
    pub fn map_lines(
        &self,
        old_content: Option<&[u8]>,
        new_content: Option<&[u8]>,
    ) -> Option<FrozenHunk> {
        Some(FrozenHunk {
            old: map_side(
                self.old_lines.clone(),
                self.old_content.as_deref(),
                old_content,
            )
            .ok()?,
            new: map_side(
                self.new_lines.clone(),
                self.new_content.as_deref(),
                new_content,
            )
            .ok()?,
        })
    }
}

/// Whether a source anchor targets full hunks or a narrower line range.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AnchorKind {
    #[default]
    Hunks,
    Lines,
}

fn map_side(
    interval: Option<Range<u32>>,
    source_content: Option<&[u8]>,
    current_content: Option<&[u8]>,
) -> Result<Option<Range<u32>>, ()> {
    match interval {
        None => Ok(None),
        Some(interval) => Ok(Some(
            transform_interval(
                interval,
                &histogram_edits(source_content.ok_or(())?, current_content.ok_or(())?),
            )
            .ok_or(())?,
        )),
    }
}

fn transform_interval(
    source: Range<u32>,
    edits: &[(Range<u32>, Range<u32>)],
) -> Option<Range<u32>> {
    let mut shift = 0_i64;
    for (before, after) in edits {
        if before.end <= source.start {
            shift += i64::from(after.end - after.start) - i64::from(before.end - before.start);
        } else if source.end <= before.start {
            break;
        } else {
            return None;
        }
    }
    let start = i64::from(source.start)
        .checked_add(shift)?
        .try_into()
        .ok()?;
    let end = i64::from(source.end).checked_add(shift)?.try_into().ok()?;
    Some(start..end)
}

fn histogram_edits(before: &[u8], after: &[u8]) -> Vec<(Range<u32>, Range<u32>)> {
    let input = InternedInput::new(before, after);
    Diff::compute(Algorithm::Histogram, &input)
        .hunks()
        .map(|hunk| (hunk.before, hunk.after))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkpoint_identity_requires_the_review_unit_and_snapshot() {
        let checkpoint = ReviewCheckpoint::new("review", "snapshot");
        assert!(checkpoint.matches(&ReviewUnit::from("review"), "snapshot"));
        assert!(!checkpoint.matches(&ReviewUnit::from("other"), "snapshot"));
    }

    #[test]
    fn mapping_retains_unchanged_lines_after_an_insertion() {
        let anchor = DiffRangeAnchor {
            source_checkpoint: "checkpoint".into(),
            old_path: None,
            new_path: Some("src/lib.rs".into()),
            old_lines: None,
            new_lines: Some(1..2),
            target_kind: AnchorKind::Lines,
            source_hunk_count: 1,
            old_content: None,
            new_content: Some(b"first\nsecond\n".to_vec()),
            diff_hash: String::new(),
        };
        assert_eq!(
            anchor.map_new_lines(b"inserted\nfirst\nsecond\n"),
            Some(2..3)
        );
        assert_eq!(anchor.map_new_lines(b"first\nchanged\n"), None);
    }
}
