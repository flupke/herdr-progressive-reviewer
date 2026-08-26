//! Shared review-guide types and validation.

use std::collections::HashMap;
use std::ops::Range;

use gix_imara_diff::{Algorithm, Diff, InternedInput};
use serde::{Deserialize, Serialize};

/// The repository identity for which one guide was generated.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReviewCheckpoint {
    pub review_unit: String,
    pub checkpoint: String,
}

impl ReviewCheckpoint {
    pub fn new(review_unit: impl Into<String>, checkpoint: impl Into<String>) -> Self {
        Self {
            review_unit: review_unit.into(),
            checkpoint: checkpoint.into(),
        }
    }

    pub fn matches(&self, review_unit: &str, checkpoint: &str) -> bool {
        self.review_unit == review_unit && self.checkpoint == checkpoint
    }
}

/// The stable identifier for one guide request.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct GuideRequestId(String);

impl GuideRequestId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Display for GuideRequestId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

/// The files included in one guide request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum GuideScope {
    /// Only the selected file.
    File { path: String },
    /// Every visible unreviewed file.
    All,
}

/// A validated target within one frozen diff.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum GuideTarget {
    /// One inclusive range of request-local text hunks.
    Hunks {
        path: String,
        first_hunk: usize,
        last_hunk: usize,
    },
    /// One line range on either or both sides of a text diff.
    Lines {
        path: String,
        old: Option<GuideLineRange>,
        new: Option<GuideLineRange>,
    },
    /// A changed file that has no text hunk.
    File { path: String },
}

impl GuideTarget {
    /// Return the repository-relative target path.
    pub fn path(&self) -> &str {
        match self {
            Self::Hunks { path, .. } | Self::Lines { path, .. } | Self::File { path } => path,
        }
    }
}

/// One inclusive, one-based line range in a guide response.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GuideLineRange {
    pub first_line: u32,
    pub last_line: u32,
}

impl GuideLineRange {
    fn half_open(&self) -> Option<Range<u32>> {
        (self.first_line > 0 && self.first_line <= self.last_line)
            .then(|| self.first_line - 1..self.last_line)
    }

    fn from_half_open(range: Range<u32>) -> Self {
        Self {
            first_line: range.start.saturating_add(1),
            last_line: range.end,
        }
    }
}

/// Whether a guide item targets its generation checkpoint or a later checkpoint.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuideItemStatus {
    /// The item targets the checkpoint for which the guide was generated.
    #[default]
    Matched,
    /// The item was conservatively carried to a later checkpoint.
    Stale,
}

/// One concise explanation and its target.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GuideItem {
    pub target: GuideTarget,
    pub text: String,
    #[serde(default)]
    pub status: GuideItemStatus,
}

/// One complete accepted guide response.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GuideSnapshot {
    pub schema_version: u8,
    #[serde(flatten)]
    pub review_checkpoint: ReviewCheckpoint,
    pub request_id: GuideRequestId,
    pub scope: GuideScope,
    pub items: Vec<GuideItem>,
    #[serde(default)]
    pub anchored_items: Vec<AnchoredGuideItem>,
}

/// A file section in a frozen guide request.
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

/// The two line intervals in one frozen text hunk.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FrozenHunk {
    pub old: Option<Range<u32>>,
    pub new: Option<Range<u32>>,
}

/// One guide explanation with its immutable source anchor.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AnchoredGuideItem {
    pub text: String,
    pub anchor: DiffRangeAnchor,
}

/// An immutable source range used for direct checkpoint-to-current mapping.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DiffRangeAnchor {
    pub source_checkpoint: String,
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    pub old_lines: Option<Range<u32>>,
    pub new_lines: Option<Range<u32>>,
    #[serde(default)]
    pub target_kind: GuideAnchorKind,
    pub source_hunk_count: usize,
    pub old_content: Option<Vec<u8>>,
    pub new_content: Option<Vec<u8>>,
    pub diff_hash: String,
}

/// The request target that produced one stored line anchor.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuideAnchorKind {
    /// A complete range of diff hunks.
    #[default]
    Hunks,
    /// A smaller range of lines within diff hunks.
    Lines,
}

/// The JSON envelope written by the implementation agent.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct GuideResponse {
    pub schema_version: u8,
    pub request_id: String,
    pub items: Vec<GuideItem>,
}

/// A response envelope cannot be accepted.
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("the guide response uses an unsupported schema version")]
    Schema,
    #[error("the guide response has the wrong request ID")]
    Request,
}

/// A valid response and the number of rejected individual items.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedResponse {
    pub items: Vec<GuideItem>,
    pub rejected_items: usize,
}

impl GuideResponse {
    /// Validate an agent response against its exact frozen request.
    pub fn validate(
        self,
        request_id: &str,
        files: &[FrozenFile],
    ) -> Result<ValidatedResponse, ValidationError> {
        if self.schema_version != 1 {
            return Err(ValidationError::Schema);
        }
        if self.request_id != request_id {
            return Err(ValidationError::Request);
        }
        let files = files
            .iter()
            .map(|file| (file.path.as_str(), file))
            .collect::<HashMap<_, _>>();
        let mut ranges: HashMap<String, Vec<TargetRanges>> = HashMap::new();
        let mut accepted = Vec::new();
        let mut rejected_items = 0;
        for mut item in self.items {
            item.text = item.text.split_whitespace().collect::<Vec<_>>().join(" ");
            item.status = GuideItemStatus::Matched;
            let valid = !item.text.is_empty()
                && match &item.target {
                    GuideTarget::Hunks {
                        path,
                        first_hunk,
                        last_hunk,
                    } => {
                        let Some(file) = files.get(path.as_str()).copied() else {
                            rejected_items += 1;
                            continue;
                        };
                        if *first_hunk == 0
                            || first_hunk > last_hunk
                            || *last_hunk > file.hunk_count
                        {
                            false
                        } else {
                            let selected = &file.hunks[first_hunk - 1..*last_hunk];
                            accept_non_overlapping(
                                &mut ranges,
                                path,
                                TargetRanges {
                                    old: enclosing_range(
                                        selected.iter().filter_map(|hunk| hunk.old.clone()),
                                    ),
                                    new: enclosing_range(
                                        selected.iter().filter_map(|hunk| hunk.new.clone()),
                                    ),
                                },
                            )
                        }
                    }
                    GuideTarget::Lines { path, old, new } => {
                        let Some(file) = files.get(path.as_str()).copied() else {
                            rejected_items += 1;
                            continue;
                        };
                        line_target_ranges(file, old.as_ref(), new.as_ref())
                            .is_some_and(|target| accept_non_overlapping(&mut ranges, path, target))
                    }
                    GuideTarget::File { path } => files
                        .get(path.as_str())
                        .is_some_and(|file| file.hunk_count == 0),
                };
            if valid {
                accepted.push(item);
            } else {
                rejected_items += 1;
            }
        }
        Ok(ValidatedResponse {
            items: accepted,
            rejected_items,
        })
    }
}

#[derive(Clone, Debug)]
struct TargetRanges {
    old: Option<Range<u32>>,
    new: Option<Range<u32>>,
}

fn accept_non_overlapping(
    ranges: &mut HashMap<String, Vec<TargetRanges>>,
    path: &str,
    target: TargetRanges,
) -> bool {
    let existing = ranges.entry(path.to_owned()).or_default();
    let overlaps = existing.iter().any(|other| {
        ranges_overlap(other.old.as_ref(), target.old.as_ref())
            || ranges_overlap(other.new.as_ref(), target.new.as_ref())
    });
    if !overlaps {
        existing.push(target);
    }
    !overlaps
}

fn line_target_ranges(
    file: &FrozenFile,
    old: Option<&GuideLineRange>,
    new: Option<&GuideLineRange>,
) -> Option<TargetRanges> {
    let old = match old {
        Some(range) => Some(range.half_open()?),
        None => None,
    };
    let new = match new {
        Some(range) => Some(range.half_open()?),
        None => None,
    };
    if old.is_none() && new.is_none() {
        return None;
    }
    let old_hunks = covered_hunks(
        old.as_ref(),
        file.hunks.iter().map(|hunk| hunk.old.as_ref()),
    )?;
    let new_hunks = covered_hunks(
        new.as_ref(),
        file.hunks.iter().map(|hunk| hunk.new.as_ref()),
    )?;
    if old_hunks != CoveredHunks::NotRequested
        && new_hunks != CoveredHunks::NotRequested
        && old_hunks != new_hunks
    {
        return None;
    }
    Some(TargetRanges { old, new })
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum CoveredHunks {
    NotRequested,
    Range(Range<usize>),
}

fn covered_hunks<'a>(
    target: Option<&Range<u32>>,
    hunk_ranges: impl Iterator<Item = Option<&'a Range<u32>>>,
) -> Option<CoveredHunks> {
    let Some(target) = target else {
        return Some(CoveredHunks::NotRequested);
    };
    let matching = hunk_ranges
        .enumerate()
        .filter_map(|(index, range)| ranges_overlap(range, Some(target)).then_some((index, range?)))
        .collect::<Vec<_>>();
    let first_index = matching.first()?.0;
    let last_index = matching.last()?.0;
    let mut covered_until = target.start;
    for (_, range) in matching {
        if range.start > covered_until {
            return None;
        }
        covered_until = covered_until.max(range.end.min(target.end));
    }
    (covered_until >= target.end)
        .then(|| CoveredHunks::Range(first_index..last_index.saturating_add(1)))
}

/// Replace a path scope or the complete guide with a new response.
pub fn replace_items(
    previous: &[GuideItem],
    scope: &GuideScope,
    replacement: Vec<GuideItem>,
) -> Vec<GuideItem> {
    replace_file_scope(previous, scope, replacement, |item, path| {
        item.target.path() == path
    })
}

/// Replace source anchors for one path scope or for the complete guide.
pub fn replace_anchored_items(
    previous: &[AnchoredGuideItem],
    scope: &GuideScope,
    replacement: Vec<AnchoredGuideItem>,
    current_files: &[FrozenFile],
) -> Vec<AnchoredGuideItem> {
    replace_file_scope(previous, scope, replacement, |item, path| {
        current_files
            .iter()
            .filter(|file| file.path == path)
            .any(|file| anchor_belongs_to_file(item, file))
    })
}

fn anchor_belongs_to_file(item: &AnchoredGuideItem, current: &FrozenFile) -> bool {
    let source_paths = [item.anchor.old_path.as_ref(), item.anchor.new_path.as_ref()];
    let current_paths = [current.old_path.as_ref(), current.new_path.as_ref()];
    source_paths.into_iter().flatten().any(|source| {
        current_paths
            .into_iter()
            .flatten()
            .any(|current| source == current)
    })
}

fn replace_file_scope<T: Clone>(
    previous: &[T],
    scope: &GuideScope,
    mut replacement: Vec<T>,
    belongs_to_path: impl Fn(&T, &str) -> bool,
) -> Vec<T> {
    if let GuideScope::File { path } = scope {
        replacement.extend(
            previous
                .iter()
                .filter(|item| !belongs_to_path(item, path))
                .cloned(),
        );
    }
    replacement
}

/// Replace request-local hunk labels with immutable source anchors.
pub fn anchor_items(
    items: &[GuideItem],
    files: &[FrozenFile],
    checkpoint: &str,
) -> Vec<AnchoredGuideItem> {
    items
        .iter()
        .filter_map(|item| {
            let path = item.target.path();
            let file = files.iter().find(|file| file.path == *path)?;
            let (old_lines, new_lines, target_kind, source_hunk_count) = match &item.target {
                GuideTarget::Hunks {
                    first_hunk,
                    last_hunk,
                    ..
                } => {
                    let selected = file.hunks.get(first_hunk - 1..*last_hunk)?;
                    (
                        enclosing_range(selected.iter().filter_map(|hunk| hunk.old.clone())),
                        enclosing_range(selected.iter().filter_map(|hunk| hunk.new.clone())),
                        GuideAnchorKind::Hunks,
                        selected.len(),
                    )
                }
                GuideTarget::Lines { old, new, .. } => {
                    let ranges = line_target_ranges(file, old.as_ref(), new.as_ref())?;
                    let touched = file
                        .hunks
                        .iter()
                        .filter(|hunk| {
                            ranges_overlap(hunk.old.as_ref(), ranges.old.as_ref())
                                || ranges_overlap(hunk.new.as_ref(), ranges.new.as_ref())
                        })
                        .count();
                    (ranges.old, ranges.new, GuideAnchorKind::Lines, touched)
                }
                GuideTarget::File { .. } => (None, None, GuideAnchorKind::Hunks, 0),
            };
            Some(AnchoredGuideItem {
                text: item.text.clone(),
                anchor: DiffRangeAnchor {
                    source_checkpoint: checkpoint.to_owned(),
                    old_path: file.old_path.clone(),
                    new_path: file.new_path.clone(),
                    old_lines,
                    new_lines,
                    target_kind,
                    source_hunk_count,
                    old_content: file.old_content.clone(),
                    new_content: file.new_content.clone(),
                    diff_hash: file.diff_hash.clone(),
                },
            })
        })
        .collect()
}

fn enclosing_range(mut ranges: impl Iterator<Item = Range<u32>>) -> Option<Range<u32>> {
    let first = ranges.next()?;
    Some(ranges.fold(first, |enclosure, range| {
        enclosure.start.min(range.start)..enclosure.end.max(range.end)
    }))
}

/// Transform one half-open source line interval through ordered edits.
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

/// Compute ordered byte-line edits for Gerrit-style position transformation.
fn histogram_edits(before: &[u8], after: &[u8]) -> Vec<(Range<u32>, Range<u32>)> {
    let input = InternedInput::new(before, after);
    Diff::compute(Algorithm::Histogram, &input)
        .hunks()
        .map(|hunk| (hunk.before, hunk.after))
        .collect()
}

/// Map one immutable source anchor to one current frozen file.
fn map_anchored_item(item: &AnchoredGuideItem, current: &FrozenFile) -> Option<GuideItem> {
    if !anchor_belongs_to_file(item, current) {
        return None;
    }
    if item.anchor.source_hunk_count == 0 {
        return (current.hunk_count == 0 && item.anchor.diff_hash == current.diff_hash).then(
            || GuideItem {
                target: GuideTarget::File {
                    path: current.path.clone(),
                },
                text: item.text.clone(),
                status: GuideItemStatus::Stale,
            },
        );
    }
    let old_lines = map_side(
        item.anchor.old_lines.clone(),
        item.anchor.old_content.as_deref(),
        current.old_content.as_deref(),
    )
    .ok()?;
    let new_lines = map_side(
        item.anchor.new_lines.clone(),
        item.anchor.new_content.as_deref(),
        current.new_content.as_deref(),
    )
    .ok()?;
    if item.anchor.target_kind == GuideAnchorKind::Lines {
        let old_hunks = covered_hunks(
            old_lines.as_ref(),
            current.hunks.iter().map(|hunk| hunk.old.as_ref()),
        )?;
        let new_hunks = covered_hunks(
            new_lines.as_ref(),
            current.hunks.iter().map(|hunk| hunk.new.as_ref()),
        )?;
        if old_hunks != CoveredHunks::NotRequested
            && new_hunks != CoveredHunks::NotRequested
            && old_hunks != new_hunks
        {
            return None;
        }
        return Some(GuideItem {
            target: GuideTarget::Lines {
                path: current.path.clone(),
                old: old_lines.map(GuideLineRange::from_half_open),
                new: new_lines.map(GuideLineRange::from_half_open),
            },
            text: item.text.clone(),
            status: GuideItemStatus::Stale,
        });
    }
    let matching = current
        .hunks
        .iter()
        .enumerate()
        .filter(|(_, hunk)| {
            ranges_overlap(hunk.old.as_ref(), old_lines.as_ref())
                || ranges_overlap(hunk.new.as_ref(), new_lines.as_ref())
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let first = *matching.first()?;
    let last = *matching.last()?;
    if matching.len() != last - first + 1 {
        return None;
    }
    let selected = &current.hunks[first..=last];
    if enclosing_range(selected.iter().filter_map(|hunk| hunk.old.clone())) != old_lines
        || enclosing_range(selected.iter().filter_map(|hunk| hunk.new.clone())) != new_lines
    {
        return None;
    }
    Some(GuideItem {
        target: GuideTarget::Hunks {
            path: current.path.clone(),
            first_hunk: first + 1,
            last_hunk: last + 1,
        },
        text: item.text.clone(),
        status: GuideItemStatus::Stale,
    })
}

/// Map an ordered guide while omitting overlaps and reordered targets.
pub fn map_anchored_items(
    items: &[AnchoredGuideItem],
    current_files: &[FrozenFile],
) -> Vec<GuideItem> {
    let mut previous_position = None;
    items
        .iter()
        .filter_map(|item| {
            let matches = current_files
                .iter()
                .enumerate()
                .filter_map(|(index, file)| map_anchored_item(item, file).map(|item| (index, item)))
                .collect::<Vec<_>>();
            let [(file_index, mapped)] = matches.as_slice() else {
                return None;
            };
            let order_range = match &mapped.target {
                GuideTarget::Hunks {
                    first_hunk,
                    last_hunk,
                    ..
                } => (*first_hunk - 1, 0)..(*last_hunk, 0),
                GuideTarget::Lines { old, new, .. } => {
                    let file = &current_files[*file_index];
                    let old_half_open = old.as_ref().and_then(GuideLineRange::half_open);
                    let new_half_open = new.as_ref().and_then(GuideLineRange::half_open);
                    let matching = file
                        .hunks
                        .iter()
                        .enumerate()
                        .filter(|(_, hunk)| {
                            ranges_overlap(hunk.old.as_ref(), old_half_open.as_ref())
                                || ranges_overlap(hunk.new.as_ref(), new_half_open.as_ref())
                        })
                        .map(|(index, _)| index)
                        .collect::<Vec<_>>();
                    let first_hunk = *matching.first()?;
                    let last_hunk = *matching.last()?;
                    let first_line = new
                        .as_ref()
                        .or(old.as_ref())
                        .map_or(0, |range| range.first_line);
                    let last_line = new
                        .as_ref()
                        .or(old.as_ref())
                        .map_or(first_line, |range| range.last_line);
                    (first_hunk, first_line)..(last_hunk, last_line.saturating_add(1))
                }
                GuideTarget::File { .. } => (0, 0)..(0, 1),
            };
            let position = (*file_index, order_range.end);
            if previous_position.is_some_and(|(previous_file, previous_end)| {
                *file_index < previous_file
                    || (*file_index == previous_file && order_range.start < previous_end)
            }) {
                return None;
            }
            previous_position = Some(position);
            Some(mapped.clone())
        })
        .collect()
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

fn ranges_overlap(left: Option<&Range<u32>>, right: Option<&Range<u32>>) -> bool {
    left.zip(right)
        .is_some_and(|(left, right)| left.start < right.end && right.start < left.end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_before_an_interval_shift_it() {
        assert_eq!(
            transform_interval(10..14, &[(2..2, 2..5), (20..21, 20..20)]),
            Some(13..17)
        );
    }

    #[test]
    fn an_overlapping_edit_hides_the_interval() {
        assert_eq!(transform_interval(10..14, &[(12..13, 12..14)]), None);
    }

    #[test]
    fn histogram_edits_preserve_missing_final_newlines() {
        assert_eq!(
            histogram_edits(b"one\ntwo", b"zero\none\ntwo"),
            vec![(0..0, 0..1)]
        );
    }

    #[test]
    fn valid_items_are_normalized_and_overlaps_are_rejected() {
        let response = GuideResponse {
            schema_version: 1,
            request_id: "request".to_owned(),
            items: vec![
                GuideItem {
                    target: GuideTarget::Hunks {
                        path: "src/lib.rs".to_owned(),
                        first_hunk: 1,
                        last_hunk: 2,
                    },
                    text: "  central   idea  ".to_owned(),
                    status: GuideItemStatus::Stale,
                },
                GuideItem {
                    target: GuideTarget::Hunks {
                        path: "src/lib.rs".to_owned(),
                        first_hunk: 2,
                        last_hunk: 2,
                    },
                    text: "overlap".to_owned(),
                    status: GuideItemStatus::Matched,
                },
            ],
        };
        let validated = response
            .validate(
                "request",
                &[FrozenFile {
                    path: "src/lib.rs".to_owned(),
                    hunk_count: 2,
                    old_path: None,
                    new_path: None,
                    old_content: None,
                    new_content: None,
                    hunks: vec![
                        FrozenHunk {
                            old: Some(0..1),
                            new: Some(0..1),
                        },
                        FrozenHunk {
                            old: Some(2..3),
                            new: Some(2..3),
                        },
                    ],
                    diff_hash: String::new(),
                }],
            )
            .unwrap();
        assert_eq!(validated.items.len(), 1);
        assert_eq!(validated.items[0].text, "central idea");
        assert_eq!(validated.items[0].status, GuideItemStatus::Matched);
        assert_eq!(validated.rejected_items, 1);
    }

    #[test]
    fn invalid_hunk_ranges_are_rejected() {
        let hunk_item = |first_hunk, last_hunk| GuideItem {
            target: GuideTarget::Hunks {
                path: "src/lib.rs".to_owned(),
                first_hunk,
                last_hunk,
            },
            text: "central idea".to_owned(),
            status: GuideItemStatus::Matched,
        };
        let response = GuideResponse {
            schema_version: 1,
            request_id: "request".to_owned(),
            items: vec![hunk_item(0, 1), hunk_item(2, 1), hunk_item(1, 3)],
        };
        let file = FrozenFile {
            path: "src/lib.rs".to_owned(),
            hunk_count: 2,
            old_path: None,
            new_path: None,
            old_content: None,
            new_content: None,
            hunks: Vec::new(),
            diff_hash: String::new(),
        };

        let validated = response.validate("request", &[file]).unwrap();

        assert!(validated.items.is_empty());
        assert_eq!(validated.rejected_items, 3);
    }

    #[test]
    fn separate_line_targets_are_allowed_in_one_new_file_hunk() {
        let line_target = |first_line, last_line, text: &str| GuideItem {
            target: GuideTarget::Lines {
                path: "src/lib.rs".to_owned(),
                old: None,
                new: Some(GuideLineRange {
                    first_line,
                    last_line,
                }),
            },
            text: text.to_owned(),
            status: GuideItemStatus::Matched,
        };
        let response = GuideResponse {
            schema_version: 1,
            request_id: "request".to_owned(),
            items: vec![
                line_target(1, 10, "first idea"),
                line_target(20, 30, "second idea"),
                line_target(25, 35, "overlap"),
            ],
        };
        let file = FrozenFile {
            path: "src/lib.rs".to_owned(),
            hunk_count: 1,
            old_path: None,
            new_path: Some("src/lib.rs".to_owned()),
            old_content: None,
            new_content: Some(Vec::new()),
            hunks: vec![FrozenHunk {
                old: None,
                new: Some(0..100),
            }],
            diff_hash: "diff".to_owned(),
        };

        let validated = response.validate("request", &[file]).unwrap();

        assert_eq!(validated.items.len(), 2);
        assert_eq!(validated.rejected_items, 1);
    }

    #[test]
    fn line_targets_require_one_based_ordered_ranges() {
        let line_item = |first_line, last_line| GuideItem {
            target: GuideTarget::Lines {
                path: "src/lib.rs".to_owned(),
                old: None,
                new: Some(GuideLineRange {
                    first_line,
                    last_line,
                }),
            },
            text: "central idea".to_owned(),
            status: GuideItemStatus::Matched,
        };
        let response = GuideResponse {
            schema_version: 1,
            request_id: "request".to_owned(),
            items: vec![line_item(1, 1), line_item(0, 1), line_item(2, 1)],
        };
        let file = FrozenFile {
            path: "src/lib.rs".to_owned(),
            hunk_count: 1,
            old_path: None,
            new_path: Some("src/lib.rs".to_owned()),
            old_content: None,
            new_content: Some(Vec::new()),
            hunks: vec![FrozenHunk {
                old: None,
                new: Some(0..3),
            }],
            diff_hash: String::new(),
        };

        let validated = response.validate("request", &[file]).unwrap();

        assert_eq!(validated.items, vec![line_item(1, 1)]);
        assert_eq!(validated.rejected_items, 2);
    }

    #[test]
    fn adjacent_line_targets_are_accepted_in_either_order() {
        let line_item = |path: &str, first_line, last_line| GuideItem {
            target: GuideTarget::Lines {
                path: path.to_owned(),
                old: None,
                new: Some(GuideLineRange {
                    first_line,
                    last_line,
                }),
            },
            text: "central idea".to_owned(),
            status: GuideItemStatus::Matched,
        };
        let response = GuideResponse {
            schema_version: 1,
            request_id: "request".to_owned(),
            items: vec![
                line_item("ascending.rs", 1, 10),
                line_item("ascending.rs", 11, 20),
                line_item("descending.rs", 11, 20),
                line_item("descending.rs", 1, 10),
            ],
        };
        let file = |path: &str| FrozenFile {
            path: path.to_owned(),
            hunk_count: 1,
            old_path: None,
            new_path: Some(path.to_owned()),
            old_content: None,
            new_content: Some(Vec::new()),
            hunks: vec![FrozenHunk {
                old: None,
                new: Some(0..20),
            }],
            diff_hash: String::new(),
        };

        let validated = response
            .validate("request", &[file("ascending.rs"), file("descending.rs")])
            .unwrap();

        assert_eq!(validated.items.len(), 4);
        assert_eq!(validated.rejected_items, 0);
    }

    fn assert_added_file_line_target_maps(
        source_content: &[u8],
        source_target: GuideLineRange,
        current_content: &[u8],
        expected_target: GuideLineRange,
    ) {
        let added_file = |content: &[u8], diff_hash: &str| {
            let line_count = content
                .split_inclusive(|byte| *byte == b'\n')
                .count()
                .try_into()
                .unwrap();
            FrozenFile {
                path: "src/lib.rs".to_owned(),
                hunk_count: 1,
                old_path: None,
                new_path: Some("src/lib.rs".to_owned()),
                old_content: None,
                new_content: Some(content.to_vec()),
                hunks: vec![FrozenHunk {
                    old: None,
                    new: Some(0..line_count),
                }],
                diff_hash: diff_hash.to_owned(),
            }
        };
        let source = added_file(source_content, "source");
        let item = GuideItem {
            target: GuideTarget::Lines {
                path: source.path.clone(),
                old: None,
                new: Some(source_target),
            },
            text: "central idea".to_owned(),
            status: GuideItemStatus::Matched,
        };
        let [anchored] = anchor_items(&[item], &[source], "checkpoint")
            .try_into()
            .unwrap();
        let current = added_file(current_content, "current");

        let mapped = map_anchored_item(&anchored, &current).unwrap();

        assert_eq!(
            mapped.target,
            GuideTarget::Lines {
                path: "src/lib.rs".to_owned(),
                old: None,
                new: Some(expected_target),
            }
        );
    }

    #[test]
    fn a_line_target_moves_after_an_insertion_before_it() {
        assert_added_file_line_target_maps(
            b"one\ntwo\nthree\nfour\nfive\n",
            GuideLineRange {
                first_line: 3,
                last_line: 4,
            },
            b"zero\none\ntwo\nthree\nfour\nfive\n",
            GuideLineRange {
                first_line: 4,
                last_line: 5,
            },
        );
    }

    #[test]
    fn a_line_target_moves_after_a_deletion_before_it() {
        assert_added_file_line_target_maps(
            b"zero\none\ntwo\nthree\nfour\nfive\n",
            GuideLineRange {
                first_line: 4,
                last_line: 5,
            },
            b"two\nthree\nfour\nfive\n",
            GuideLineRange {
                first_line: 2,
                last_line: 3,
            },
        );
    }

    #[test]
    fn anchored_item_moves_after_lines_are_inserted_before_it() {
        let item = AnchoredGuideItem {
            text: "central idea".to_owned(),
            anchor: DiffRangeAnchor {
                source_checkpoint: "source".to_owned(),
                old_path: Some("src/lib.rs".to_owned()),
                new_path: Some("src/lib.rs".to_owned()),
                old_lines: Some(1..2),
                new_lines: Some(1..2),
                target_kind: GuideAnchorKind::Hunks,
                source_hunk_count: 1,
                old_content: Some(b"a\ntarget\n".to_vec()),
                new_content: Some(b"a\nchanged\n".to_vec()),
                diff_hash: "source".to_owned(),
            },
        };
        let current = FrozenFile {
            path: "src/lib.rs".to_owned(),
            hunk_count: 1,
            old_path: Some("src/lib.rs".to_owned()),
            new_path: Some("src/lib.rs".to_owned()),
            old_content: Some(b"before\na\ntarget\n".to_vec()),
            new_content: Some(b"before\na\nchanged\n".to_vec()),
            hunks: vec![FrozenHunk {
                old: Some(2..3),
                new: Some(2..3),
            }],
            diff_hash: "current".to_owned(),
        };
        let mapped = map_anchored_item(&item, &current).unwrap();
        assert_eq!(mapped.status, GuideItemStatus::Stale);
        assert_eq!(
            mapped.target,
            GuideTarget::Hunks {
                path: "src/lib.rs".to_owned(),
                first_hunk: 1,
                last_hunk: 1,
            }
        );
    }

    #[test]
    fn line_anchor_requires_both_sides_to_map_to_the_same_hunks() {
        let anchored = |old_lines, new_lines| AnchoredGuideItem {
            text: "central idea".to_owned(),
            anchor: DiffRangeAnchor {
                source_checkpoint: "source".to_owned(),
                old_path: Some("src/lib.rs".to_owned()),
                new_path: Some("src/lib.rs".to_owned()),
                old_lines,
                new_lines,
                target_kind: GuideAnchorKind::Lines,
                source_hunk_count: 2,
                old_content: Some(b"a\nb\nc\n".to_vec()),
                new_content: Some(b"a\nb\nc\n".to_vec()),
                diff_hash: "source".to_owned(),
            },
        };
        let current = FrozenFile {
            path: "src/lib.rs".to_owned(),
            hunk_count: 2,
            old_path: Some("src/lib.rs".to_owned()),
            new_path: Some("src/lib.rs".to_owned()),
            old_content: Some(b"a\nb\nc\n".to_vec()),
            new_content: Some(b"a\nb\nc\n".to_vec()),
            hunks: vec![
                FrozenHunk {
                    old: Some(0..1),
                    new: Some(0..1),
                },
                FrozenHunk {
                    old: Some(2..3),
                    new: Some(2..3),
                },
            ],
            diff_hash: "current".to_owned(),
        };

        assert!(map_anchored_item(&anchored(Some(0..1), Some(0..1)), &current).is_some());
        assert_eq!(
            map_anchored_item(&anchored(Some(0..1), Some(2..3)), &current),
            None
        );
    }

    #[test]
    fn one_sided_hunk_anchor_maps_to_an_added_hunk() {
        let item = AnchoredGuideItem {
            text: "central idea".to_owned(),
            anchor: DiffRangeAnchor {
                source_checkpoint: "source".to_owned(),
                old_path: None,
                new_path: Some("src/lib.rs".to_owned()),
                old_lines: None,
                new_lines: Some(0..1),
                target_kind: GuideAnchorKind::Hunks,
                source_hunk_count: 1,
                old_content: None,
                new_content: Some(b"new\n".to_vec()),
                diff_hash: "source".to_owned(),
            },
        };
        let current = FrozenFile {
            path: "src/lib.rs".to_owned(),
            hunk_count: 1,
            old_path: None,
            new_path: Some("src/lib.rs".to_owned()),
            old_content: None,
            new_content: Some(b"new\n".to_vec()),
            hunks: vec![FrozenHunk {
                old: None,
                new: Some(0..1),
            }],
            diff_hash: "current".to_owned(),
        };

        assert_eq!(
            map_anchored_item(&item, &current).map(|item| item.target),
            Some(GuideTarget::Hunks {
                path: "src/lib.rs".to_owned(),
                first_hunk: 1,
                last_hunk: 1,
            })
        );
    }

    #[test]
    fn hunk_anchor_maps_consecutive_hunks_after_an_unrelated_hunk() {
        let content = b"zero\none\ntwo\nthree\nfour\n".to_vec();
        let item = AnchoredGuideItem {
            text: "central idea".to_owned(),
            anchor: DiffRangeAnchor {
                source_checkpoint: "source".to_owned(),
                old_path: Some("src/lib.rs".to_owned()),
                new_path: Some("src/lib.rs".to_owned()),
                old_lines: Some(2..5),
                new_lines: Some(2..5),
                target_kind: GuideAnchorKind::Hunks,
                source_hunk_count: 2,
                old_content: Some(content.clone()),
                new_content: Some(content.clone()),
                diff_hash: "source".to_owned(),
            },
        };
        let current = FrozenFile {
            path: "src/lib.rs".to_owned(),
            hunk_count: 3,
            old_path: Some("src/lib.rs".to_owned()),
            new_path: Some("src/lib.rs".to_owned()),
            old_content: Some(content.clone()),
            new_content: Some(content),
            hunks: vec![
                FrozenHunk {
                    old: Some(0..1),
                    new: Some(0..1),
                },
                FrozenHunk {
                    old: Some(2..3),
                    new: Some(2..3),
                },
                FrozenHunk {
                    old: Some(4..5),
                    new: Some(4..5),
                },
            ],
            diff_hash: "current".to_owned(),
        };

        assert_eq!(
            map_anchored_item(&item, &current).map(|item| item.target),
            Some(GuideTarget::Hunks {
                path: "src/lib.rs".to_owned(),
                first_hunk: 2,
                last_hunk: 3,
            })
        );
    }

    #[test]
    fn hunk_anchor_rejects_an_unexplained_line_on_one_side() {
        let item = AnchoredGuideItem {
            text: "central idea".to_owned(),
            anchor: DiffRangeAnchor {
                source_checkpoint: "source".to_owned(),
                old_path: Some("src/lib.rs".to_owned()),
                new_path: Some("src/lib.rs".to_owned()),
                old_lines: Some(0..1),
                new_lines: Some(0..1),
                target_kind: GuideAnchorKind::Hunks,
                source_hunk_count: 1,
                old_content: Some(b"old\n".to_vec()),
                new_content: Some(b"new\n".to_vec()),
                diff_hash: "source".to_owned(),
            },
        };
        let current = FrozenFile {
            path: "src/lib.rs".to_owned(),
            hunk_count: 1,
            old_path: Some("src/lib.rs".to_owned()),
            new_path: Some("src/lib.rs".to_owned()),
            old_content: Some(b"old\nunexplained\n".to_vec()),
            new_content: Some(b"new\n".to_vec()),
            hunks: vec![FrozenHunk {
                old: Some(0..2),
                new: Some(0..1),
            }],
            diff_hash: "current".to_owned(),
        };

        assert_eq!(map_anchored_item(&item, &current), None);
    }

    #[test]
    fn anchors_without_paths_do_not_match_each_other() {
        let item = AnchoredGuideItem {
            text: "notice".to_owned(),
            anchor: DiffRangeAnchor {
                source_checkpoint: "source".to_owned(),
                old_path: None,
                new_path: None,
                old_lines: None,
                new_lines: None,
                target_kind: GuideAnchorKind::Hunks,
                source_hunk_count: 0,
                old_content: None,
                new_content: None,
                diff_hash: String::new(),
            },
        };
        let current = FrozenFile {
            path: "other".to_owned(),
            hunk_count: 0,
            old_path: None,
            new_path: None,
            old_content: None,
            new_content: None,
            hunks: Vec::new(),
            diff_hash: String::new(),
        };

        assert_eq!(map_anchored_item(&item, &current), None);
    }

    #[test]
    fn file_scoped_replacement_keeps_other_path_anchors() {
        let anchored = |path: &str| AnchoredGuideItem {
            text: path.to_owned(),
            anchor: DiffRangeAnchor {
                source_checkpoint: "source".to_owned(),
                old_path: Some(path.to_owned()),
                new_path: Some(path.to_owned()),
                old_lines: None,
                new_lines: None,
                target_kind: GuideAnchorKind::Hunks,
                source_hunk_count: 0,
                old_content: None,
                new_content: None,
                diff_hash: path.to_owned(),
            },
        };

        let result = replace_anchored_items(
            &[anchored("one"), anchored("two")],
            &GuideScope::File {
                path: "one".to_owned(),
            },
            vec![anchored("one")],
            &[zero_hunk_file("one", "one"), zero_hunk_file("two", "two")],
        );

        assert_eq!(result.len(), 2);
        assert!(result.iter().any(|item| item.text == "two"));
    }

    #[test]
    fn file_scoped_replacement_removes_an_anchor_mapped_through_a_rename() {
        let previous = AnchoredGuideItem {
            text: "old explanation".to_owned(),
            anchor: DiffRangeAnchor {
                source_checkpoint: "source".to_owned(),
                old_path: Some("old.rs".to_owned()),
                new_path: Some("old.rs".to_owned()),
                old_lines: None,
                new_lines: None,
                target_kind: GuideAnchorKind::Hunks,
                source_hunk_count: 0,
                old_content: None,
                new_content: None,
                diff_hash: "rename".to_owned(),
            },
        };
        let renamed = FrozenFile {
            path: "new.rs".to_owned(),
            hunk_count: 0,
            old_path: Some("old.rs".to_owned()),
            new_path: Some("new.rs".to_owned()),
            old_content: None,
            new_content: None,
            hunks: Vec::new(),
            diff_hash: "rename".to_owned(),
        };

        let result = replace_anchored_items(
            &[previous],
            &GuideScope::File {
                path: "new.rs".to_owned(),
            },
            Vec::new(),
            &[renamed],
        );

        assert!(result.is_empty());
    }

    #[test]
    fn file_scoped_replacement_removes_an_anchor_with_an_overlapping_edit() {
        let previous = AnchoredGuideItem {
            text: "outdated explanation".to_owned(),
            anchor: DiffRangeAnchor {
                source_checkpoint: "source".to_owned(),
                old_path: Some("src/lib.rs".to_owned()),
                new_path: Some("src/lib.rs".to_owned()),
                old_lines: Some(0..1),
                new_lines: Some(0..1),
                target_kind: GuideAnchorKind::Hunks,
                source_hunk_count: 1,
                old_content: Some(b"old\n".to_vec()),
                new_content: Some(b"new\n".to_vec()),
                diff_hash: "source".to_owned(),
            },
        };
        let edited = FrozenFile {
            path: "src/lib.rs".to_owned(),
            hunk_count: 1,
            old_path: Some("src/lib.rs".to_owned()),
            new_path: Some("src/lib.rs".to_owned()),
            old_content: Some(b"changed old\n".to_vec()),
            new_content: Some(b"changed new\n".to_vec()),
            hunks: vec![FrozenHunk {
                old: Some(0..1),
                new: Some(0..1),
            }],
            diff_hash: "current".to_owned(),
        };
        assert_eq!(map_anchored_item(&previous, &edited), None);

        let result = replace_anchored_items(
            &[previous],
            &GuideScope::File {
                path: "src/lib.rs".to_owned(),
            },
            Vec::new(),
            &[edited],
        );

        assert!(result.is_empty());
    }

    fn zero_hunk_file(path: &str, diff_hash: &str) -> FrozenFile {
        FrozenFile {
            path: path.to_owned(),
            hunk_count: 0,
            old_path: Some(path.to_owned()),
            new_path: Some(path.to_owned()),
            old_content: None,
            new_content: None,
            hunks: Vec::new(),
            diff_hash: diff_hash.to_owned(),
        }
    }

    #[test]
    fn changed_zero_hunk_file_does_not_keep_its_guide() {
        let item = AnchoredGuideItem {
            text: "binary format".to_owned(),
            anchor: DiffRangeAnchor {
                source_checkpoint: "source".to_owned(),
                old_path: Some("image.png".to_owned()),
                new_path: Some("image.png".to_owned()),
                old_lines: None,
                new_lines: None,
                target_kind: GuideAnchorKind::Hunks,
                source_hunk_count: 0,
                old_content: None,
                new_content: None,
                diff_hash: "old diff".to_owned(),
            },
        };
        let current = FrozenFile {
            path: "image.png".to_owned(),
            hunk_count: 0,
            old_path: Some("image.png".to_owned()),
            new_path: Some("image.png".to_owned()),
            old_content: None,
            new_content: None,
            hunks: Vec::new(),
            diff_hash: "new diff".to_owned(),
        };

        assert_eq!(map_anchored_item(&item, &current), None);
    }

    #[test]
    fn unchanged_zero_hunk_file_keeps_its_guide() {
        let item = AnchoredGuideItem {
            text: "binary format".to_owned(),
            anchor: DiffRangeAnchor {
                source_checkpoint: "source".to_owned(),
                old_path: Some("image.png".to_owned()),
                new_path: Some("image.png".to_owned()),
                old_lines: None,
                new_lines: None,
                target_kind: GuideAnchorKind::Hunks,
                source_hunk_count: 0,
                old_content: None,
                new_content: None,
                diff_hash: "same diff".to_owned(),
            },
        };

        assert_eq!(
            map_anchored_item(&item, &zero_hunk_file("image.png", "same diff"))
                .map(|item| item.target),
            Some(GuideTarget::File {
                path: "image.png".to_owned(),
            })
        );
    }
}
