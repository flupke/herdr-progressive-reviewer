//! Shared review-guide types and validation.

use std::collections::HashMap;
use std::ops::Range;

use gix_imara_diff::{Algorithm, Diff, InternedInput};
use review_types::ReviewUnit;
use serde::{Deserialize, Serialize};

/// The repository identity for which one guide was generated.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
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
    pub items: Vec<GuideItem>,
}

/// A response envelope cannot be accepted.
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("the guide response uses an unsupported schema version")]
    Schema,
}

/// A valid response and the number of rejected individual items.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedResponse {
    pub items: Vec<GuideItem>,
    pub rejected_items: usize,
}

impl GuideResponse {
    /// Validate an agent response against its exact frozen request.
    pub fn validate(self, files: &[FrozenFile]) -> Result<ValidatedResponse, ValidationError> {
        if self.schema_version != 1 {
            return Err(ValidationError::Schema);
        }
        let mut validator = ResponseValidator::new(files);
        let mut accepted = Vec::new();
        let mut rejected_items = 0;
        for mut item in self.items {
            item.text = item.text.split_whitespace().collect::<Vec<_>>().join(" ");
            item.status = GuideItemStatus::Matched;
            if !item.text.is_empty() && validator.accept(&item.target) {
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

struct ResponseValidator<'a> {
    files: HashMap<&'a str, &'a FrozenFile>,
    ranges: HashMap<String, Vec<TargetRanges>>,
}

impl<'a> ResponseValidator<'a> {
    fn new(files: &'a [FrozenFile]) -> Self {
        Self {
            files: files
                .iter()
                .map(|file| (file.path.as_str(), file))
                .collect(),
            ranges: HashMap::new(),
        }
    }

    fn accept(&mut self, target: &GuideTarget) -> bool {
        match target {
            GuideTarget::Hunks {
                path,
                first_hunk,
                last_hunk,
            } => self.accept_hunks(path, *first_hunk, *last_hunk),
            GuideTarget::Lines { path, old, new } => {
                self.accept_lines(path, old.as_ref(), new.as_ref())
            }
            GuideTarget::File { path } => self
                .files
                .get(path.as_str())
                .is_some_and(|file| file.hunk_count == 0),
        }
    }

    fn accept_hunks(&mut self, path: &str, first_hunk: usize, last_hunk: usize) -> bool {
        let Some(file) = self.files.get(path).copied() else {
            return false;
        };
        if first_hunk == 0 || first_hunk > last_hunk || last_hunk > file.hunk_count {
            return false;
        }
        let selected = &file.hunks[first_hunk - 1..last_hunk];
        accept_non_overlapping(
            &mut self.ranges,
            path,
            TargetRanges {
                old: enclosing_range(selected.iter().filter_map(|hunk| hunk.old.clone())),
                new: enclosing_range(selected.iter().filter_map(|hunk| hunk.new.clone())),
            },
        )
    }

    fn accept_lines(
        &mut self,
        path: &str,
        old: Option<&GuideLineRange>,
        new: Option<&GuideLineRange>,
    ) -> bool {
        self.files
            .get(path)
            .and_then(|file| line_target_ranges(file, old, new))
            .is_some_and(|target| accept_non_overlapping(&mut self.ranges, path, target))
    }
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
        return map_line_item(item, current, old_lines, new_lines);
    }
    map_hunk_item(item, current, old_lines.as_ref(), new_lines.as_ref())
}

fn map_line_item(
    item: &AnchoredGuideItem,
    current: &FrozenFile,
    old_lines: Option<Range<u32>>,
    new_lines: Option<Range<u32>>,
) -> Option<GuideItem> {
    let old_hunks = covered_hunks(
        old_lines.as_ref(),
        current.hunks.iter().map(|hunk| hunk.old.as_ref()),
    )?;
    let new_hunks = covered_hunks(
        new_lines.as_ref(),
        current.hunks.iter().map(|hunk| hunk.new.as_ref()),
    )?;
    let both_requested =
        old_hunks != CoveredHunks::NotRequested && new_hunks != CoveredHunks::NotRequested;
    if both_requested && old_hunks != new_hunks {
        return None;
    }
    Some(GuideItem {
        target: GuideTarget::Lines {
            path: current.path.clone(),
            old: old_lines.map(GuideLineRange::from_half_open),
            new: new_lines.map(GuideLineRange::from_half_open),
        },
        text: item.text.clone(),
        status: GuideItemStatus::Stale,
    })
}

fn map_hunk_item(
    item: &AnchoredGuideItem,
    current: &FrozenFile,
    old_lines: Option<&Range<u32>>,
    new_lines: Option<&Range<u32>>,
) -> Option<GuideItem> {
    let matching = current
        .hunks
        .iter()
        .enumerate()
        .filter(|(_, hunk)| {
            ranges_overlap(hunk.old.as_ref(), old_lines)
                || ranges_overlap(hunk.new.as_ref(), new_lines)
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let first = *matching.first()?;
    let last = *matching.last()?;
    if matching.len() != last - first + 1 {
        return None;
    }
    let selected = &current.hunks[first..=last];
    if enclosing_range(selected.iter().filter_map(|hunk| hunk.old.clone())).as_ref() != old_lines
        || enclosing_range(selected.iter().filter_map(|hunk| hunk.new.clone())).as_ref()
            != new_lines
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
#[path = "lib.tests.rs"]
mod tests;
