//! Jev's decisions about the changes of one comparison: which changes it
//! classified, and which changed lines it found too insignificant to need
//! review.
use review_explore::{Comparison, SourceSide};
use review_repository::{
    diff::{DiffRow, NoticeKind, parse_file_diff},
    repository::{ChangeKind, FileKind},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Intervals are one-based and half-open. Each unit belongs to exactly one changed file.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChangeUnit {
    Lines {
        file: usize,
        side: SourceSide,
        first: u32,
        end: u32,
    },
    Item {
        file: usize,
        name: String,
    },
}

impl ChangeUnit {
    pub fn file_index(&self) -> usize {
        match self {
            Self::Lines { file, .. } | Self::Item { file, .. } => *file,
        }
    }
}

/// Every change of a comparison: its changed lines and its non-text changes.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ChangeInventory {
    pub units: Vec<ChangeUnit>,
    /// Whether every change could be listed.
    pub complete: bool,
}

/// One file's changed lines that Jev excluded, one-based like diff rows:
/// old-side lines of the base and new-side lines of the current file.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ExcludedLines(Vec<(SourceSide, std::ops::Range<u32>)>);

impl ExcludedLines {
    pub fn contains(&self, side: SourceSide, line: u32) -> bool {
        self.0
            .iter()
            .any(|(excluded_side, lines)| *excluded_side == side && lines.contains(&line))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Significance {
    Significant,
    Insignificant,
    Uncertain,
    Failed,
    Oversized,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct SignificanceResult {
    pub id: String,
    pub units: Vec<ChangeUnit>,
    pub outcome: Significance,
    pub model: Option<String>,
    pub rubric: String,
    #[serde(default)]
    pub criterion: String,
    pub input_references: Vec<String>,
    pub omissions: Vec<String>,
    pub probabilities: BTreeMap<String, f64>,
    pub confidence: Option<f64>,
    pub error: Option<String>,
}

impl Eq for SignificanceResult {}

impl ChangeInventory {
    fn file_complete(&self, comparison: &Comparison, index: usize) -> bool {
        if self.complete {
            return true;
        }
        let (Some(file), Some(diff)) = (comparison.files.get(index), comparison.diffs.get(index))
        else {
            return false;
        };
        let mut inventory = Self {
            complete: true,
            ..Self::default()
        };
        inventory.capture_file(index, file, diff);
        inventory.complete
    }

    fn capture(comparison: &Comparison) -> Self {
        let mut result = Self {
            complete: comparison.diffs.len() == comparison.files.len(),
            ..Self::default()
        };
        if !result.complete {
            return result;
        }
        for (file_index, file) in comparison.files.iter().enumerate() {
            result.capture_file(file_index, file, &comparison.diffs[file_index]);
        }
        result.units = normalize(result.units);
        result
    }

    fn capture_file(
        &mut self,
        index: usize,
        file: &review_repository::repository::ChangedFile,
        diff: &[u8],
    ) {
        let mut items = BTreeSet::new();
        let has_lines = parse_file_diff(diff, file)
            .into_iter()
            .fold(false, |had, row| {
                had | self.record_row(index, row, &mut items)
            });
        self.finish_file(index, file, diff, has_lines, items);
    }

    fn finish_file(
        &mut self,
        index: usize,
        file: &review_repository::repository::ChangedFile,
        diff: &[u8],
        has_lines: bool,
        mut items: BTreeSet<String>,
    ) {
        items.extend(Self::metadata_items(file));
        if !has_lines
            && items.is_empty()
            && matches!(file.change, ChangeKind::Added | ChangeKind::Deleted)
        {
            items.insert("empty_file".to_owned());
        }
        if !has_lines && items.is_empty() && diff.is_empty() {
            self.complete = false;
        }
        self.units.extend(
            items
                .into_iter()
                .map(|name| ChangeUnit::Item { file: index, name }),
        );
    }

    fn metadata_items(file: &review_repository::repository::ChangedFile) -> BTreeSet<String> {
        let mut items = BTreeSet::new();
        if file.old_path != file.new_path && file.old_path.is_some() && file.new_path.is_some() {
            items.insert("rename".to_owned());
        }
        if file.old_kind != FileKind::Absent
            && file.new_kind != FileKind::Absent
            && file.old_kind != file.new_kind
        {
            items.insert("type".to_owned());
        }
        if matches!(file.old_kind, FileKind::Gitlink) || matches!(file.new_kind, FileKind::Gitlink)
        {
            items.insert("submodule".to_owned());
        }
        items
    }

    fn record_row(&mut self, index: usize, row: DiffRow, items: &mut BTreeSet<String>) -> bool {
        match row {
            DiffRow::Add { new_line, .. } => {
                self.units.push(ChangeUnit::Lines {
                    file: index,
                    side: SourceSide::New,
                    first: new_line,
                    end: new_line.saturating_add(1),
                });
                true
            }
            DiffRow::Delete { old_line, .. } => {
                self.units.push(ChangeUnit::Lines {
                    file: index,
                    side: SourceSide::Old,
                    first: old_line,
                    end: old_line.saturating_add(1),
                });
                true
            }
            DiffRow::Meta { text }
                if text.starts_with("old mode ") || text.starts_with("new mode ") =>
            {
                items.insert("mode".into());
                false
            }
            DiffRow::Notice {
                kind: NoticeKind::Binary,
                ..
            } => {
                items.insert("binary".into());
                false
            }
            DiffRow::Notice {
                kind: NoticeKind::Unsupported | NoticeKind::Conflict,
                ..
            } => {
                self.complete = false;
                false
            }
            _ => false,
        }
    }
}

/// What Jev decided about the changes of one comparison.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SignificanceLedger {
    inventory: ChangeInventory,
    classifications: BTreeMap<String, SignificanceResult>,
    excluded: Vec<ChangeUnit>,
}

impl SignificanceLedger {
    pub fn new(comparison: &Comparison) -> Self {
        Self {
            inventory: ChangeInventory::capture(comparison),
            ..Self::default()
        }
    }

    /// Record one classification of changes in the inventory; false when it
    /// was recorded already or names other changes.
    pub fn record_significance(&mut self, result: SignificanceResult) -> bool {
        if self.classifications.contains_key(&result.id)
            || result.units.is_empty()
            || !subtract(&result.units, &self.inventory.units).is_empty()
        {
            return false;
        }
        if result.outcome == Significance::Insignificant {
            self.excluded = normalize(
                self.excluded
                    .iter()
                    .cloned()
                    .chain(result.units.iter().cloned())
                    .collect(),
            );
        }
        self.classifications.insert(result.id.clone(), result);
        true
    }

    /// Every change of the comparison.
    pub fn inventory(&self) -> &ChangeInventory {
        &self.inventory
    }

    pub fn classifications(&self) -> impl Iterator<Item = &SignificanceResult> {
        self.classifications.values()
    }

    /// The changed lines of one file that Jev excluded.
    pub fn excluded_lines(&self, file: usize) -> ExcludedLines {
        ExcludedLines(
            self.excluded
                .iter()
                .filter_map(|unit| match unit {
                    ChangeUnit::Lines {
                        file: unit_file,
                        side,
                        first,
                        end,
                    } if *unit_file == file => Some((*side, *first..*end)),
                    _ => None,
                })
                .collect(),
        )
    }

    /// Whether a file changes more than its text lines (a mode, type or
    /// path change), or has changes the inventory cannot list.
    pub fn changes_more_than_lines(&self, comparison: &Comparison, file: usize) -> bool {
        !self.inventory.file_complete(comparison, file)
            || self
                .inventory
                .units
                .iter()
                .any(|unit| matches!(unit, ChangeUnit::Item { file: unit_file, .. } if *unit_file == file))
    }

    /// Files whose entire listed change Jev excluded. Files the inventory
    /// cannot list completely never qualify.
    pub fn fully_excluded_files(&self, comparison: &Comparison) -> Vec<usize> {
        (0..comparison.files.len())
            .filter(|file| {
                let units = self
                    .inventory
                    .units
                    .iter()
                    .filter(|unit| unit.file_index() == *file)
                    .cloned()
                    .collect::<Vec<_>>();
                !units.is_empty()
                    && subtract(&units, &self.excluded).is_empty()
                    && self.inventory.file_complete(comparison, *file)
            })
            .collect()
    }
}

fn normalize(mut units: Vec<ChangeUnit>) -> Vec<ChangeUnit> {
    units.sort();
    let mut result: Vec<ChangeUnit> = Vec::new();
    for unit in units {
        if let (
            Some(ChangeUnit::Lines {
                file: left_file,
                side: left_side,
                first: _,
                end: left_end,
            }),
            ChangeUnit::Lines {
                file,
                side,
                first,
                end,
            },
        ) = (result.last_mut(), &unit)
            && left_file == file
            && left_side == side
            && *first <= *left_end
        {
            *left_end = (*left_end).max(*end);
        } else if result.last() != Some(&unit) {
            result.push(unit);
        }
    }
    result
}

fn subtract(left: &[ChangeUnit], right: &[ChangeUnit]) -> Vec<ChangeUnit> {
    let mut result = Vec::new();
    for unit in left {
        match unit {
            ChangeUnit::Item { .. } => {
                if !right.contains(unit) {
                    result.push(unit.clone());
                }
            }
            ChangeUnit::Lines {
                file,
                side,
                first,
                end,
            } => {
                let mut pieces = vec![(*first, *end)];
                for cut in right {
                    if let ChangeUnit::Lines {
                        file: other_file,
                        side: other_side,
                        first: cut0,
                        end: cut1,
                    } = cut
                        && file == other_file
                        && side == other_side
                    {
                        pieces = pieces
                            .into_iter()
                            .flat_map(|(a, b)| {
                                if *cut1 <= a || *cut0 >= b {
                                    vec![(a, b)]
                                } else {
                                    [(a, (*cut0).min(b)), ((*cut1).max(a), b)]
                                        .into_iter()
                                        .filter(|(x, y)| x < y)
                                        .collect()
                                }
                            })
                            .collect();
                    }
                }
                result.extend(pieces.into_iter().map(|(first, end)| ChangeUnit::Lines {
                    file: *file,
                    side: *side,
                    first,
                    end,
                }));
            }
        }
    }
    normalize(result)
}

#[cfg(test)]
#[path = "ledger.tests.rs"]
mod tests;
