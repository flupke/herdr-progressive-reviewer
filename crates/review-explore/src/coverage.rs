//! Compact, checkpoint-bound accounting for changed lines and non-text changes.
use crate::{CodeLocation, Comparison, EvidenceRef, ReviewerAnswer, SourceSide};
use review_guide::GuideLineRange;
use review_repository::{
    diff::{DiffRow, NoticeKind, parse_file_diff},
    repository::{ChangeKind, FileKind, RepoPath},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Intervals are one-based and half-open. Each unit belongs to exactly one changed file.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CoverageUnit {
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

impl CoverageUnit {
    pub fn file_index(&self) -> usize {
        match self {
            Self::Lines { file, .. } | Self::Item { file, .. } => *file,
        }
    }

    fn weight(&self) -> u64 {
        match self {
            Self::Lines { first, end, .. } => u64::from(end - first),
            Self::Item { .. } => 1,
        }
    }

    fn changed_line_weight(&self) -> u64 {
        match self {
            Self::Lines { .. } => self.weight(),
            Self::Item { .. } => 0,
        }
    }
}

/// Added and deleted diff lines, counted on their respective sides.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ChangedLineCoverage {
    pub explored: u64,
    pub total: u64,
}

impl ChangedLineCoverage {
    /// Percentage in tenths, truncated so partial coverage never appears complete.
    pub fn percent_tenths(self) -> Option<u16> {
        (self.total > 0).then(|| {
            let tenths = u128::from(self.explored) * 1000 / u128::from(self.total);
            u16::try_from(tenths.min(1000)).unwrap_or(1000)
        })
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
pub struct CoverageInventory {
    pub units: Vec<CoverageUnit>,
    pub limitations: Vec<String>,
    /// Old passes have no trustworthy geometry and cannot be completed.
    pub complete: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
pub struct CoverageLedger {
    pub inventory: CoverageInventory,
    /// Exact answer IDs, each with its immutable credited intersection.
    pub answers: BTreeMap<String, Vec<CoverageUnit>>,
    pub credited: Vec<CoverageUnit>,
    /// Jev results never erase raw answer coverage.
    pub excluded: Vec<CoverageUnit>,
    pub required_overrides: Vec<CoverageUnit>,
    pub revision: u64,
    /// Changes only when data used by coverage counts changes.
    #[serde(default)]
    pub counts_revision: u64,
    pub classification_started: bool,
    pub classification_attempt: Option<String>,
    #[serde(default)]
    pub classification_rubric: Option<String>,
    pub classifications: BTreeMap<String, SignificanceResult>,
    #[serde(default)]
    pub jev_elapsed_ms: u64,
    #[serde(default)]
    pub classification_finished: bool,
    /// Wall-clock time when the latest filtering attempt stopped, successful or not.
    #[serde(default)]
    pub classification_stopped_at_ms: Option<u64>,
    #[serde(default)]
    pub jev_total_windows: usize,
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
    pub units: Vec<CoverageUnit>,
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

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum GapKind {
    Lines,
    Item(String),
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct Gap {
    pub location: CodeLocation,
    pub kind: GapKind,
    pub question: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
pub struct CoverageSummary {
    pub complete: bool,
    pub total: u64,
    pub required: u64,
    pub explored_required: u64,
    pub excluded_unexplored: u64,
    pub remaining: u64,
    pub percent: Option<u8>,
    pub limitations: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct FileCoverage {
    pub file: usize,
    pub path: RepoPath,
    pub summary: CoverageSummary,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct CoverageFeedback {
    pub revision: u64,
    pub summary: CoverageSummary,
    /// Percentage of required changed text lines answered, in tenths.
    #[serde(default)]
    pub covered_percent_tenths: Option<u16>,
    /// Jev-filtered locations still requiring an answer, grouped for quick inspection.
    #[serde(default)]
    pub uncovered: Box<UncoveredOverview>,
    pub total_gaps: usize,
    pub has_more: bool,
    pub unassigned_required: Vec<Gap>,
    pub awaiting_answer: Vec<Gap>,
    pub jev: JevFeedback,
}

/// Saved tool feedback distinguishes a question's projection from actual coverage.
/// The unwrapped current variant also reads receipts saved before projections existed.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(untagged)]
pub enum CoverageReceipt {
    AfterAnswer {
        coverage_after_answer: CoverageFeedback,
    },
    Current(CoverageFeedback),
}

impl CoverageReceipt {
    pub fn feedback(&self) -> &CoverageFeedback {
        match self {
            Self::AfterAnswer {
                coverage_after_answer,
            } => coverage_after_answer,
            Self::Current(coverage) => coverage,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
pub struct UncoveredOverview {
    pub total_files: usize,
    pub files: Vec<UncoveredArea>,
    pub files_truncated: bool,
    pub directories: Vec<UncoveredArea>,
    pub directories_truncated: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct UncoveredArea {
    pub path: String,
    pub files: usize,
    pub remaining: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum JevMode {
    Enabled,
    Disabled,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct JevFeedback {
    pub mode: JevMode,
    pub excluded_unexplored: usize,
    pub pending_or_unclassified: usize,
}

impl CoverageInventory {
    fn capture(comparison: &Comparison) -> Self {
        let mut result = Self {
            complete: comparison.diffs.len() == comparison.files.len(),
            ..Self::default()
        };
        if !result.complete {
            result
                .limitations
                .push("Saved comparison has no complete diff inventory; start a New pass".into());
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
                had | self.record_row(index, file, row, &mut items)
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
            self.limitations.push(format!(
                "{}: change has no enumerable diff",
                file.review_path().display()
            ));
        }
        self.units.extend(
            items
                .into_iter()
                .map(|name| CoverageUnit::Item { file: index, name }),
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

    fn record_row(
        &mut self,
        index: usize,
        file: &review_repository::repository::ChangedFile,
        row: DiffRow,
        items: &mut BTreeSet<String>,
    ) -> bool {
        match row {
            DiffRow::Add { new_line, .. } => {
                self.units.push(CoverageUnit::Lines {
                    file: index,
                    side: SourceSide::New,
                    first: new_line,
                    end: new_line.saturating_add(1),
                });
                true
            }
            DiffRow::Delete { old_line, .. } => {
                self.units.push(CoverageUnit::Lines {
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
                text,
            } => {
                self.complete = false;
                self.limitations
                    .push(format!("{}: {text}", file.review_path().display()));
                false
            }
            _ => false,
        }
    }
}

impl CoverageLedger {
    pub fn needs_classification(&self, rubric: &str, conversation_revision: u64) -> bool {
        if self.classification_started {
            self.classification_rubric.as_deref() != Some(rubric) || !self.classification_finished
        } else {
            conversation_revision == 0
        }
    }

    pub fn new(comparison: &Comparison) -> Self {
        Self {
            inventory: CoverageInventory::capture(comparison),
            ..Self::default()
        }
    }

    pub(crate) fn validate_restored(
        &self,
        comparison: &Comparison,
        answer_ids: impl Iterator<Item = String>,
    ) -> eyre::Result<()> {
        let ids: BTreeSet<_> = answer_ids.collect();
        for unit in &self.inventory.units {
            eyre::ensure!(
                unit.file_index() < comparison.files.len(),
                "coverage references an unknown file"
            );
            if let CoverageUnit::Lines {
                file,
                side,
                first,
                end,
            } = unit
            {
                let changed_file = &comparison.files[*file];
                eyre::ensure!(
                    *first > 0
                        && first < end
                        && match side {
                            SourceSide::Old => changed_file.old_path.is_some(),
                            SourceSide::New => changed_file.new_path.is_some(),
                        },
                    "invalid saved changed-line interval"
                );
            }
        }
        eyre::ensure!(
            normalize(self.inventory.units.clone()) == self.inventory.units,
            "coverage inventory is not normalized"
        );
        eyre::ensure!(
            subtract(&self.credited, &self.inventory.units).is_empty()
                && subtract(&self.excluded, &self.inventory.units).is_empty()
                && subtract(&self.required_overrides, &self.inventory.units).is_empty(),
            "coverage references changes outside the inventory"
        );
        let mut attributed = Vec::new();
        for (id, units) in &self.answers {
            eyre::ensure!(
                ids.contains(id) && subtract(units, &self.inventory.units).is_empty(),
                "saved coverage lost its answer or changed region"
            );
            attributed.extend(units.iter().cloned());
        }
        eyre::ensure!(
            normalize(attributed) == self.credited,
            "saved answer coverage is inconsistent"
        );
        for result in self.classifications.values() {
            eyre::ensure!(
                subtract(&result.units, &self.inventory.units).is_empty(),
                "classification references changes outside the inventory"
            );
        }
        Ok(())
    }

    pub(crate) fn credit(&mut self, answer: &ReviewerAnswer, comparison: &Comparison) {
        if answer.deferred || self.answers.contains_key(&answer.id) {
            return;
        }
        let Some(question) = &answer.question else {
            return;
        };
        let cited = self.credit_question(question, comparison);
        self.answers.insert(answer.id.clone(), cited);
        self.revision += 1;
    }

    fn credit_question(
        &mut self,
        question: &crate::Question,
        comparison: &Comparison,
    ) -> Vec<CoverageUnit> {
        let cited = self.intersections(
            question.evidence.iter().chain(&question.supporting),
            comparison,
        );
        let credited = normalize(
            self.credited
                .iter()
                .cloned()
                .chain(cited.iter().cloned())
                .collect(),
        );
        if self.credited != credited {
            self.credited = credited;
            self.counts_revision += 1;
        }
        cited
    }

    /// Project one non-deferred answer using current Jev results, without saving credit.
    /// The revision identifies the inputs to the projection, not a future ledger revision.
    pub(crate) fn feedback_after_answer(
        &self,
        comparison: &Comparison,
        question: &crate::Question,
        exclusions_enabled: bool,
    ) -> CoverageFeedback {
        let mut projected = self.clone();
        projected.credit_question(question, comparison);
        projected.feedback(comparison, &[], exclusions_enabled)
    }

    pub fn record_significance(&mut self, result: SignificanceResult) -> bool {
        if self.classifications.contains_key(&result.id)
            || result.units.is_empty()
            || !subtract(&result.units, &self.inventory.units).is_empty()
        {
            return false;
        }
        if result.outcome == Significance::Insignificant {
            let excluded = normalize(
                self.excluded
                    .iter()
                    .cloned()
                    .chain(result.units.iter().cloned())
                    .collect(),
            );
            if self.excluded != excluded {
                self.excluded = excluded;
                self.counts_revision += 1;
            }
        }
        self.classifications.insert(result.id.clone(), result);
        self.revision += 1;
        true
    }

    pub fn restart_classification(&mut self, rubric: &str, attempt: String) {
        let new_rubric = self.classification_rubric.as_deref() != Some(rubric);
        self.classification_started = true;
        self.classification_attempt = Some(attempt);
        self.classification_rubric = Some(rubric.into());
        if new_rubric {
            self.classifications.clear();
            if !self.excluded.is_empty() {
                self.excluded.clear();
                self.counts_revision += 1;
            }
            self.jev_elapsed_ms = 0;
        }
        self.classification_finished = false;
        self.classification_stopped_at_ms = None;
        self.revision += 1;
    }

    pub fn finish_classification(&mut self, finished: bool, elapsed_ms: u64) {
        self.jev_elapsed_ms = elapsed_ms;
        self.classification_finished = finished;
        self.classification_stopped_at_ms = Some(
            u64::try_from(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis(),
            )
            .unwrap_or(u64::MAX),
        );
        self.revision += 1;
    }

    pub fn require_review(&mut self, units: Vec<CoverageUnit>) {
        let overrides = normalize(
            self.required_overrides
                .iter()
                .cloned()
                .chain(units)
                .collect(),
        );
        if self.required_overrides != overrides {
            self.required_overrides = overrides;
            self.counts_revision += 1;
        }
        self.revision += 1;
    }

    pub fn unexplored_exclusions(&self) -> Vec<CoverageUnit> {
        subtract(
            &subtract(&self.excluded, &self.required_overrides),
            &self.credited,
        )
    }

    pub fn exclusion_gaps(&self, comparison: &Comparison) -> Vec<Gap> {
        self.unexplored_exclusions()
            .iter()
            .map(|unit| Self::gap(unit, comparison, None))
            .collect()
    }

    fn intersections<'a>(
        &self,
        references: impl Iterator<Item = &'a EvidenceRef>,
        comparison: &Comparison,
    ) -> Vec<CoverageUnit> {
        let mut result = Vec::new();
        for reference in references {
            let location = &reference.location;
            for (file_index, file) in comparison.files.iter().enumerate() {
                let path = match location.side {
                    SourceSide::Old => &file.old_path,
                    SourceSide::New => &file.new_path,
                };
                if path.as_ref() != Some(&location.path) {
                    continue;
                }
                for unit in self
                    .inventory
                    .units
                    .iter()
                    .filter(|unit| unit.file_index() == file_index)
                {
                    result.extend(Self::intersect_location(unit, location));
                }
            }
        }
        normalize(result)
    }

    fn intersect_location(unit: &CoverageUnit, location: &CodeLocation) -> Option<CoverageUnit> {
        match (unit, &location.lines) {
            (
                CoverageUnit::Lines {
                    file,
                    side,
                    first,
                    end,
                },
                Some(range),
            ) if *side == location.side => {
                let first = (*first).max(range.first_line);
                let end = (*end).min(range.last_line.saturating_add(1));
                (first < end).then_some(CoverageUnit::Lines {
                    file: *file,
                    side: *side,
                    first,
                    end,
                })
            }
            (CoverageUnit::Item { .. }, None) => Some(unit.clone()),
            _ => None,
        }
    }

    pub fn remaining(&self, exclusions_enabled: bool) -> Vec<CoverageUnit> {
        subtract(&self.required_units(exclusions_enabled), &self.credited)
    }

    fn required_units(&self, exclusions_enabled: bool) -> Vec<CoverageUnit> {
        let excluded = if exclusions_enabled {
            subtract(&self.excluded, &self.required_overrides)
        } else {
            Vec::new()
        };
        subtract(&self.inventory.units, &excluded)
    }

    pub fn summary(&self, exclusions_enabled: bool) -> CoverageSummary {
        self.summary_for(&self.inventory.units, exclusions_enabled)
    }

    /// Raw line progress ignores metadata and does not credit Jev exclusions.
    pub fn changed_line_coverage(&self, file: Option<usize>) -> ChangedLineCoverage {
        let count = |units: &[CoverageUnit]| {
            units
                .iter()
                .filter(|unit| file.is_none_or(|index| unit.file_index() == index))
                .map(CoverageUnit::changed_line_weight)
                .sum()
        };
        ChangedLineCoverage {
            explored: count(&self.credited),
            total: count(&self.inventory.units),
        }
    }

    /// Answer progress among changed lines still requiring review after Jev filtering.
    pub fn required_changed_line_coverage(
        &self,
        file: Option<usize>,
        exclusions_enabled: bool,
    ) -> ChangedLineCoverage {
        let required = self.required_units(exclusions_enabled);
        let count = |units: &[CoverageUnit]| {
            units
                .iter()
                .filter(|unit| file.is_none_or(|index| unit.file_index() == index))
                .map(CoverageUnit::changed_line_weight)
                .sum()
        };
        ChangedLineCoverage {
            explored: count(&intersect(&required, &self.credited)),
            total: count(&required),
        }
    }

    /// Count all files from one required/answered intersection, in comparison order.
    pub fn required_changed_line_counts(
        &self,
        comparison: &Comparison,
        exclusions_enabled: bool,
    ) -> Vec<ChangedLineCoverage> {
        let required = self.required_units(exclusions_enabled);
        let explored = intersect(&required, &self.credited);
        let mut files = vec![ChangedLineCoverage::default(); comparison.files.len()];
        for unit in &required {
            files[unit.file_index()].total += unit.changed_line_weight();
        }
        for unit in &explored {
            files[unit.file_index()].explored += unit.changed_line_weight();
        }
        files
    }

    /// Changed text lines currently removed from required review by Jev.
    pub fn jev_filtered_changed_lines(&self, exclusions_enabled: bool) -> u64 {
        if !exclusions_enabled {
            return 0;
        }
        let excluded = subtract(&self.excluded, &self.required_overrides);
        intersect(&self.inventory.units, &excluded)
            .iter()
            .map(CoverageUnit::changed_line_weight)
            .sum()
    }

    fn summary_for(&self, units: &[CoverageUnit], exclusions_enabled: bool) -> CoverageSummary {
        let excluded = if exclusions_enabled {
            subtract(&self.excluded, &self.required_overrides)
        } else {
            Vec::new()
        };
        let required = subtract(units, &excluded);
        let explored = intersect(&required, &self.credited);
        let excluded_unexplored = subtract(&intersect(units, &excluded), &self.credited);
        let total = weight(units);
        let required_count = weight(&required);
        let explored_count = weight(&explored);
        let remaining = required_count - explored_count;
        let percent = (self.inventory.complete && required_count > 0).then(|| {
            if remaining == 0 {
                100
            } else {
                ((explored_count.saturating_mul(100) / required_count).min(99)) as u8
            }
        });
        CoverageSummary {
            complete: self.inventory.complete,
            total,
            required: required_count,
            explored_required: explored_count,
            excluded_unexplored: weight(&excluded_unexplored),
            remaining,
            percent,
            limitations: self.inventory.limitations.clone(),
        }
    }

    pub fn files(&self, comparison: &Comparison, exclusions_enabled: bool) -> Vec<FileCoverage> {
        comparison
            .files
            .iter()
            .enumerate()
            .map(|(index, file)| {
                let units: Vec<_> = self
                    .inventory
                    .units
                    .iter()
                    .filter(|unit| unit.file_index() == index)
                    .cloned()
                    .collect();
                FileCoverage {
                    file: index,
                    path: file.review_path().clone(),
                    summary: self.summary_for(&units, exclusions_enabled),
                }
            })
            .collect()
    }

    pub fn feedback(
        &self,
        comparison: &Comparison,
        pending: &[crate::Question],
        exclusions_enabled: bool,
    ) -> CoverageFeedback {
        const LIMIT: usize = 64;
        let remaining = self.remaining(exclusions_enabled);
        let assigned = normalize(
            pending
                .iter()
                .flat_map(|question| {
                    self.intersections(
                        question.evidence.iter().chain(&question.supporting),
                        comparison,
                    )
                })
                .collect(),
        );
        let waiting = intersect(&remaining, &assigned);
        let unassigned = subtract(&remaining, &assigned);
        let total_gaps = waiting.len() + unassigned.len();
        let uncovered = Self::uncovered_overview(&remaining, comparison);
        let unassigned_required = unassigned
            .iter()
            .take(LIMIT)
            .map(|unit| Self::gap(unit, comparison, None))
            .collect();
        let awaiting_answer = waiting
            .iter()
            .take(LIMIT.saturating_sub(unassigned.len().min(LIMIT)))
            .map(|unit| {
                let question = pending
                    .iter()
                    .find(|question| {
                        self.intersections(
                            question.evidence.iter().chain(&question.supporting),
                            comparison,
                        )
                        .iter()
                        .any(|cited| {
                            !intersect(std::slice::from_ref(unit), std::slice::from_ref(cited))
                                .is_empty()
                        })
                    })
                    .map(|question| format!("{}/v{}", question.id, question.version));
                Self::gap(unit, comparison, question)
            })
            .collect();
        CoverageFeedback {
            revision: self.revision,
            summary: self.summary(exclusions_enabled),
            covered_percent_tenths: self
                .inventory
                .complete
                .then(|| {
                    self.required_changed_line_coverage(None, exclusions_enabled)
                        .percent_tenths()
                })
                .flatten(),
            uncovered: Box::new(uncovered),
            total_gaps,
            has_more: total_gaps > LIMIT,
            unassigned_required,
            awaiting_answer,
            jev: self.jev_feedback(exclusions_enabled),
        }
    }

    fn uncovered_overview(
        remaining: &[CoverageUnit],
        comparison: &Comparison,
    ) -> UncoveredOverview {
        const FILE_LIMIT: usize = 16;
        const DIRECTORY_LIMIT: usize = 32;
        let mut by_file = BTreeMap::<usize, u64>::new();
        for unit in remaining {
            *by_file.entry(unit.file_index()).or_default() += unit.weight();
        }
        let mut by_directory = BTreeMap::<String, (usize, u64)>::new();
        let mut files = Vec::with_capacity(by_file.len());
        for (index, remaining) in by_file {
            let Some(file) = comparison.files.get(index) else {
                continue;
            };
            let path = file.review_path();
            let directory = Self::directory_group(path);
            let entry = by_directory.entry(directory).or_default();
            entry.0 += 1;
            entry.1 += remaining;
            files.push(UncoveredArea {
                path: path.display(),
                files: 1,
                remaining,
            });
        }
        let total_files = files.len();
        files.sort_by(|a, b| b.remaining.cmp(&a.remaining).then(a.path.cmp(&b.path)));
        let files_truncated = files.len() > FILE_LIMIT;
        files.truncate(FILE_LIMIT);
        let mut directories: Vec<_> = by_directory
            .into_iter()
            .map(|(path, (files, remaining))| UncoveredArea {
                path,
                files,
                remaining,
            })
            .collect();
        directories.sort_by(|a, b| b.remaining.cmp(&a.remaining).then(a.path.cmp(&b.path)));
        let directories_truncated = directories.len() > DIRECTORY_LIMIT;
        directories.truncate(DIRECTORY_LIMIT);
        UncoveredOverview {
            total_files,
            files,
            files_truncated,
            directories,
            directories_truncated,
        }
    }

    /// Group nested paths by their first two components; keep root and shallow paths readable.
    fn directory_group(path: &RepoPath) -> String {
        let bytes = path.as_bytes();
        let Some(first_slash) = bytes.iter().position(|byte| *byte == b'/') else {
            return ".".into();
        };
        let end = bytes[first_slash + 1..]
            .iter()
            .position(|byte| *byte == b'/')
            .map_or(first_slash, |second| first_slash + 1 + second);
        RepoPath::from_bytes(bytes[..end].to_vec()).display()
    }

    fn jev_feedback(&self, enabled: bool) -> JevFeedback {
        if !enabled {
            return JevFeedback {
                mode: JevMode::Disabled,
                excluded_unexplored: 0,
                pending_or_unclassified: 0,
            };
        }
        let classified = normalize(
            self.classifications
                .values()
                .filter(|result| {
                    matches!(
                        result.outcome,
                        Significance::Significant
                            | Significance::Insignificant
                            | Significance::Uncertain
                    )
                })
                .flat_map(|result| result.units.iter().cloned())
                .collect(),
        );
        JevFeedback {
            mode: JevMode::Enabled,
            excluded_unexplored: self.unexplored_exclusions().len(),
            pending_or_unclassified: subtract(&self.inventory.units, &classified).len(),
        }
    }

    fn gap(unit: &CoverageUnit, comparison: &Comparison, question: Option<String>) -> Gap {
        let file = &comparison.files[unit.file_index()];
        match unit {
            CoverageUnit::Lines {
                side, first, end, ..
            } => Gap {
                location: CodeLocation {
                    path: match side {
                        SourceSide::Old => file.old_path.clone(),
                        SourceSide::New => file.new_path.clone(),
                    }
                    .expect("changed side"),
                    side: *side,
                    lines: Some(GuideLineRange {
                        first_line: *first,
                        last_line: end - 1,
                    }),
                },
                kind: GapKind::Lines,
                question,
            },
            CoverageUnit::Item { name, .. } => {
                let (path, side) = file
                    .new_path
                    .as_ref()
                    .map(|path| (path.clone(), SourceSide::New))
                    .or_else(|| {
                        file.old_path
                            .as_ref()
                            .map(|path| (path.clone(), SourceSide::Old))
                    })
                    .expect("changed path");
                Gap {
                    location: CodeLocation {
                        path,
                        side,
                        lines: None,
                    },
                    kind: GapKind::Item(name.clone()),
                    question,
                }
            }
        }
    }
}

fn weight(units: &[CoverageUnit]) -> u64 {
    units.iter().map(CoverageUnit::weight).sum()
}

fn normalize(mut units: Vec<CoverageUnit>) -> Vec<CoverageUnit> {
    units.sort();
    let mut result: Vec<CoverageUnit> = Vec::new();
    for unit in units {
        if let (
            Some(CoverageUnit::Lines {
                file: left_file,
                side: left_side,
                first: _,
                end: left_end,
            }),
            CoverageUnit::Lines {
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

fn intersect(left: &[CoverageUnit], right: &[CoverageUnit]) -> Vec<CoverageUnit> {
    let mut result = Vec::new();
    for a in left {
        for b in right {
            match (a, b) {
                (
                    CoverageUnit::Lines {
                        file: af,
                        side: as_,
                        first: a0,
                        end: a1,
                    },
                    CoverageUnit::Lines {
                        file: bf,
                        side: bs,
                        first: b0,
                        end: b1,
                    },
                ) if af == bf && as_ == bs => {
                    let first = (*a0).max(*b0);
                    let end = (*a1).min(*b1);
                    if first < end {
                        result.push(CoverageUnit::Lines {
                            file: *af,
                            side: *as_,
                            first,
                            end,
                        });
                    }
                }
                (CoverageUnit::Item { .. }, CoverageUnit::Item { .. }) if a == b => {
                    result.push(a.clone());
                }
                _ => {}
            }
        }
    }
    normalize(result)
}

fn subtract(left: &[CoverageUnit], right: &[CoverageUnit]) -> Vec<CoverageUnit> {
    let mut result = Vec::new();
    for unit in left {
        match unit {
            CoverageUnit::Item { .. } => {
                if !right.contains(unit) {
                    result.push(unit.clone());
                }
            }
            CoverageUnit::Lines {
                file,
                side,
                first,
                end,
            } => {
                let mut pieces = vec![(*first, *end)];
                for cut in right {
                    if let CoverageUnit::Lines {
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
                result.extend(pieces.into_iter().map(|(first, end)| CoverageUnit::Lines {
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
mod tests {
    use super::*;
    use crate::{Alternative, Question, TopicStatus};
    use review_repository::repository::{ChangeKind, ChangedFile, DiffStatistics};

    #[test]
    fn replacing_an_old_rubric_recalculates_exclusions_without_losing_answers() {
        let unit = CoverageUnit::Lines {
            file: 0,
            side: SourceSide::New,
            first: 1,
            end: 2,
        };
        let mut ledger = CoverageLedger {
            inventory: CoverageInventory {
                units: vec![unit.clone()],
                complete: true,
                ..CoverageInventory::default()
            },
            credited: vec![unit.clone()],
            required_overrides: vec![unit.clone()],
            ..CoverageLedger::default()
        };
        assert!(ledger.needs_classification("rubric-v1", 0));
        assert!(!ledger.needs_classification("rubric-v1", 1));
        ledger.restart_classification("rubric-v1", "attempt-v1".into());
        assert!(ledger.needs_classification("rubric-v1", 1));
        assert!(ledger.needs_classification("rubric-v2", 1));
        assert!(ledger.record_significance(SignificanceResult {
            id: "f0-b0".into(),
            units: vec![unit.clone()],
            outcome: Significance::Insignificant,
            model: Some("jev".into()),
            rubric: "rubric-v1".into(),
            criterion: String::new(),
            input_references: vec![],
            omissions: vec![],
            probabilities: BTreeMap::new(),
            confidence: None,
            error: None,
        }));
        assert_eq!(ledger.excluded, vec![unit.clone()]);

        ledger.jev_elapsed_ms = 120;
        ledger.restart_classification("rubric-v1", "resumed-attempt".into());
        assert_eq!(ledger.classifications.len(), 1);
        assert_eq!(ledger.excluded, vec![unit.clone()]);
        assert_eq!(ledger.jev_elapsed_ms, 120);
        ledger.classification_finished = true;
        assert!(!ledger.needs_classification("rubric-v1", 1));

        ledger.restart_classification("rubric-v2", "attempt-v2".into());
        assert!(ledger.classifications.is_empty());
        assert!(ledger.excluded.is_empty());
        assert_eq!(ledger.credited, vec![unit.clone()]);
        assert_eq!(ledger.required_overrides, vec![unit]);
        assert_eq!(ledger.classification_rubric.as_deref(), Some("rubric-v2"));
        assert_eq!(ledger.classification_attempt.as_deref(), Some("attempt-v2"));
        assert_eq!(ledger.jev_elapsed_ms, 0);
    }

    fn comparison(diff: &str, old: Option<&str>, new: Option<&str>) -> Comparison {
        Comparison {
            repository_root: "/tmp".into(),
            checkpoint: review_guide::ReviewCheckpoint::new("review", "aabb"),
            files: vec![ChangedFile {
                old_path: old.map(|path| RepoPath::from_bytes(path.as_bytes())),
                new_path: new.map(|path| RepoPath::from_bytes(path.as_bytes())),
                old_kind: if old.is_some() {
                    FileKind::File
                } else {
                    FileKind::Absent
                },
                new_kind: if new.is_some() {
                    FileKind::File
                } else {
                    FileKind::Absent
                },
                change: if old.is_none() {
                    ChangeKind::Added
                } else if new.is_none() {
                    ChangeKind::Deleted
                } else if old != new {
                    ChangeKind::Renamed
                } else {
                    ChangeKind::Modified
                },
                display_path: new.or(old).unwrap().into(),
                statistics: DiffStatistics::default(),
            }],
            context: Vec::new(),
            diffs: vec![diff.as_bytes().to_vec()],
            manifest: Vec::new(),
            sources: Vec::new(),
            base: None,
        }
    }

    #[test]
    fn saved_review_override_survives_a_rubric_refresh_without_an_exclusion() {
        let comparison = comparison(
            "diff --git a/f.rs b/f.rs\n--- a/f.rs\n+++ b/f.rs\n@@ -1 +1 @@\n-old\n+new\n",
            Some("f.rs"),
            Some("f.rs"),
        );
        let mut ledger = CoverageLedger::new(&comparison);
        let unit = ledger.inventory.units[1].clone();
        ledger.excluded = vec![unit.clone()];
        ledger.require_review(vec![unit.clone()]);
        ledger.restart_classification("rubric-v2", "attempt-v2".into());

        assert!(ledger.excluded.is_empty());
        assert_eq!(ledger.required_overrides, vec![unit]);
        ledger
            .validate_restored(&comparison, std::iter::empty())
            .expect("saved override is independent of the current Jev exclusions");
    }

    fn reference(path: &str, side: SourceSide, lines: Option<(u32, u32)>) -> EvidenceRef {
        EvidenceRef {
            location: CodeLocation {
                path: RepoPath::from_bytes(path.as_bytes()),
                side,
                lines: lines.map(|(first_line, last_line)| GuideLineRange {
                    first_line,
                    last_line,
                }),
            },
            relationship: "Changes the rule".into(),
            decision_relevance: "Affects choice".into(),
        }
    }

    fn answer(
        id: &str,
        evidence: Vec<EvidenceRef>,
        supporting: Vec<EvidenceRef>,
        deferred: bool,
    ) -> ReviewerAnswer {
        ReviewerAnswer {
            id: id.into(),
            checkpoint: review_guide::ReviewCheckpoint::new("review", "aabb"),
            question: Some(Question {
                id: "q".into(),
                version: 1,
                topic: "topic".into(),
                text: "Which?".into(),
                rationale: None,
                visual: None,
                alternatives: vec![
                    Alternative {
                        id: "yes".into(),
                        text: "Yes".into(),
                        outcome: TopicStatus::Accepted,
                        recommendation: None,
                    },
                    Alternative {
                        id: "no".into(),
                        text: "No".into(),
                        outcome: TopicStatus::NeedsFollowUp,
                        recommendation: None,
                    },
                ],
                evidence,
                supporting,
                assessments: None,
            }),
            in_reply_to: "turn".into(),
            option: None,
            text: "Please change it".into(),
            deferred,
            corrects: None,
            author: "reviewer".into(),
        }
    }

    #[test]
    fn changed_rows_not_hunk_headers_are_counted_on_both_sides() {
        let comparison = comparison(
            "diff --git a/a.rs b/a.rs\n@@ -1,4 +1,4 @@\n unchanged\n-old\n+new\n context\n end\n",
            Some("a.rs"),
            Some("a.rs"),
        );
        let mut ledger = CoverageLedger::new(&comparison);
        assert!(ledger.inventory.complete);
        assert_eq!(ledger.summary(false).required, 2);
        ledger.credit(
            &answer(
                "a",
                vec![reference("a.rs", SourceSide::New, Some((2, 2)))],
                vec![reference("unrelated.rs", SourceSide::New, Some((1, 1)))],
                false,
            ),
            &comparison,
        );
        assert_eq!(ledger.summary(false).explored_required, 1);
        assert_eq!(ledger.summary(false).percent, Some(50));
        assert_eq!(
            ledger.remaining(false),
            vec![CoverageUnit::Lines {
                file: 0,
                side: SourceSide::Old,
                first: 2,
                end: 3
            }]
        );
        ledger.credit(
            &answer(
                "b",
                vec![reference("a.rs", SourceSide::Old, Some((2, 2)))],
                vec![],
                true,
            ),
            &comparison,
        );
        assert_eq!(ledger.summary(false).remaining, 1, "Defer adds no credit");
        ledger.credit(
            &answer(
                "c",
                vec![reference("a.rs", SourceSide::Old, Some((2, 2)))],
                vec![],
                false,
            ),
            &comparison,
        );
        ledger.credit(
            &answer(
                "c",
                vec![reference("a.rs", SourceSide::Old, Some((2, 2)))],
                vec![],
                false,
            ),
            &comparison,
        );
        assert_eq!(ledger.summary(false).remaining, 0);
        assert_eq!(ledger.summary(false).percent, Some(100));
    }

    #[test]
    fn supporting_ranges_cover_only_changed_lines_and_metadata_needs_file_evidence() {
        let comparison = comparison(
            "diff --git a/old.rs b/new.rs\nold mode 100644\nnew mode 100755\nrename from old.rs\nrename to new.rs\n@@ -1,2 +1,2 @@\n-old\n+new\n context\n",
            Some("old.rs"),
            Some("new.rs"),
        );
        let mut ledger = CoverageLedger::new(&comparison);
        assert_eq!(
            ledger.summary(false).required,
            4,
            "one old line, one new line, rename, mode"
        );
        ledger.credit(
            &answer(
                "a",
                vec![reference("new.rs", SourceSide::New, Some((1, 2)))],
                vec![reference("old.rs", SourceSide::Old, Some((1, 1)))],
                false,
            ),
            &comparison,
        );
        assert_eq!(ledger.summary(false).explored_required, 2);
        assert_eq!(
            ledger.changed_line_coverage(None),
            ChangedLineCoverage {
                explored: 2,
                total: 2,
            },
            "renames and mode changes do not enter the line percentage"
        );
        ledger.credit(
            &answer(
                "b",
                vec![reference("new.rs", SourceSide::New, None)],
                vec![],
                false,
            ),
            &comparison,
        );
        assert_eq!(ledger.summary(false).remaining, 0);
    }

    #[test]
    fn exclusions_do_not_count_as_explored_and_require_review_restores_gap() {
        let comparison = comparison(
            "diff --git a/a.rs b/a.rs\n@@ -0,0 +1,3 @@\n+one\n+two\n+three\n",
            None,
            Some("a.rs"),
        );
        let mut ledger = CoverageLedger::new(&comparison);
        ledger.credit(
            &answer(
                "a",
                vec![reference("a.rs", SourceSide::New, Some((1, 1)))],
                vec![],
                false,
            ),
            &comparison,
        );
        let excluded = CoverageUnit::Lines {
            file: 0,
            side: SourceSide::New,
            first: 2,
            end: 4,
        };
        ledger.excluded = vec![excluded.clone()];
        assert_eq!(
            ledger.changed_line_coverage(None),
            ChangedLineCoverage {
                explored: 1,
                total: 3,
            },
            "raw changed-line coverage retains the unfiltered denominator"
        );
        assert_eq!(
            ledger.required_changed_line_coverage(None, true),
            ChangedLineCoverage {
                explored: 1,
                total: 1,
            },
        );
        assert_eq!(
            ledger.changed_line_coverage(Some(0)).percent_tenths(),
            Some(333)
        );
        assert_eq!(ledger.jev_filtered_changed_lines(true), 2);
        assert_eq!(ledger.jev_filtered_changed_lines(false), 0);
        let summary = ledger.summary(true);
        assert_eq!(
            (
                summary.required,
                summary.explored_required,
                summary.excluded_unexplored,
                summary.percent
            ),
            (1, 1, 2, Some(100))
        );
        ledger.require_review(vec![excluded]);
        assert_eq!(ledger.jev_filtered_changed_lines(true), 0);
        let summary = ledger.summary(true);
        assert_eq!(
            (summary.required, summary.remaining, summary.percent),
            (3, 2, Some(33))
        );
        assert_eq!(ledger.summary(false).remaining, 2);
    }

    #[test]
    fn feedback_excludes_jev_from_coverage_denominator_and_lists_only_required_files() {
        let mut changed = comparison(
            "diff --git a/crates/one/a.rs b/crates/one/a.rs\n@@ -0,0 +1,2 @@\n+one\n+two\n",
            None,
            Some("crates/one/a.rs"),
        );
        let second = comparison(
            "diff --git a/crates/two/b.rs b/crates/two/b.rs\n@@ -0,0 +1,2 @@\n+three\n+four\n",
            None,
            Some("crates/two/b.rs"),
        );
        changed.files.extend(second.files);
        changed.diffs.extend(second.diffs);
        let mut ledger = CoverageLedger::new(&changed);
        ledger.credit(
            &answer(
                "a",
                vec![reference("crates/one/a.rs", SourceSide::New, Some((1, 1)))],
                vec![],
                false,
            ),
            &changed,
        );
        ledger.excluded = vec![
            CoverageUnit::Lines {
                file: 0,
                side: SourceSide::New,
                first: 1,
                end: 3,
            },
            CoverageUnit::Lines {
                file: 1,
                side: SourceSide::New,
                first: 1,
                end: 2,
            },
        ];

        let enabled = ledger.feedback(&changed, &[], true);
        assert_eq!(enabled.covered_percent_tenths, Some(0));
        assert_eq!(
            ledger.required_changed_line_coverage(Some(0), true),
            ChangedLineCoverage {
                explored: 0,
                total: 0,
            },
        );
        assert_eq!(enabled.summary.remaining, 1);
        assert_eq!(enabled.uncovered.total_files, 1);
        assert_eq!(enabled.uncovered.files[0].path, "crates/two/b.rs");
        assert_eq!(enabled.uncovered.files[0].remaining, 1);
        assert_eq!(enabled.uncovered.directories[0].path, "crates/two");
        let mut legacy_receipt = serde_json::to_value(&enabled).unwrap();
        legacy_receipt
            .as_object_mut()
            .unwrap()
            .remove("covered_percent_tenths");
        legacy_receipt.as_object_mut().unwrap().remove("uncovered");
        let restored: CoverageFeedback = serde_json::from_value(legacy_receipt).unwrap();
        assert_eq!(restored.covered_percent_tenths, None);
        assert_eq!(restored.uncovered.total_files, 0);

        let disabled = ledger.feedback(&changed, &[], false);
        assert_eq!(disabled.covered_percent_tenths, Some(250));
        assert_eq!(disabled.uncovered.total_files, 2);
        assert_eq!(disabled.uncovered.directories.len(), 2);

        ledger.require_review(vec![CoverageUnit::Lines {
            file: 1,
            side: SourceSide::New,
            first: 1,
            end: 2,
        }]);
        let required_again = ledger.feedback(&changed, &[], true);
        assert_eq!(required_again.covered_percent_tenths, Some(0));
        assert_eq!(required_again.uncovered.files[0].remaining, 2);

        ledger.require_review(vec![CoverageUnit::Lines {
            file: 0,
            side: SourceSide::New,
            first: 1,
            end: 2,
        }]);
        assert_eq!(
            ledger.feedback(&changed, &[], true).covered_percent_tenths,
            Some(333)
        );
        assert_eq!(
            ledger.required_changed_line_coverage(Some(0), true),
            ChangedLineCoverage {
                explored: 1,
                total: 1,
            },
        );
    }

    #[test]
    fn projected_feedback_matches_answer_credit_without_changing_the_ledger() {
        let mut changed = comparison(
            "diff --git a/src/a.rs b/src/a.rs\n@@ -1 +1,3 @@\n-old\n+new\n+more\n+tail\n",
            Some("src/a.rs"),
            Some("src/a.rs"),
        );
        let filtered = comparison(
            "diff --git a/other/b.rs b/other/b.rs\n@@ -0,0 +1 @@\n+ignored\n",
            None,
            Some("other/b.rs"),
        );
        changed.files.extend(filtered.files);
        changed.diffs.extend(filtered.diffs);
        let mut ledger = CoverageLedger::new(&changed);
        ledger.excluded = vec![
            CoverageUnit::Lines {
                file: 0,
                side: SourceSide::New,
                first: 2,
                end: 3,
            },
            CoverageUnit::Lines {
                file: 1,
                side: SourceSide::New,
                first: 1,
                end: 2,
            },
        ];
        let answer = answer(
            "future",
            vec![reference("src/a.rs", SourceSide::New, Some((1, 2)))],
            vec![
                reference("src/a.rs", SourceSide::Old, Some((1, 1))),
                reference("src/a.rs", SourceSide::New, Some((2, 2))),
            ],
            false,
        );
        for enabled in [true, false] {
            let before = ledger.clone();
            let projected =
                ledger.feedback_after_answer(&changed, answer.question.as_ref().unwrap(), enabled);
            assert_eq!(ledger, before, "projection must not credit a real answer");
            assert!(projected.awaiting_answer.is_empty());
            assert_eq!(
                projected.covered_percent_tenths,
                Some(if enabled { 666 } else { 600 })
            );
            assert_eq!(projected.summary.remaining, if enabled { 1 } else { 2 });
            assert_eq!(projected.uncovered.total_files, if enabled { 1 } else { 2 });
            let mut answered = ledger.clone();
            let mut deferred = answer.clone();
            deferred.deferred = true;
            answered.credit(&deferred, &changed);
            assert_eq!(answered, ledger, "deferral never applies projected credit");
            answered.credit(&answer, &changed);
            let mut actual = answered.feedback(&changed, &[], enabled);
            actual.revision = projected.revision;
            assert_eq!(projected, actual);
            assert_eq!(
                answered.feedback_after_answer(
                    &changed,
                    answer.question.as_ref().unwrap(),
                    enabled
                ),
                answered.feedback(&changed, &[], enabled),
                "overlapping questions must not double count"
            );
        }
    }

    #[test]
    fn legacy_receipts_keep_actual_coverage_when_restored() {
        let changed = comparison("", None, Some("empty.rs"));
        let feedback = CoverageLedger::new(&changed).feedback(&changed, &[], false);
        let stored = serde_json::to_value(&feedback).unwrap();
        let restored: CoverageReceipt = serde_json::from_value(stored.clone()).unwrap();
        assert_eq!(restored, CoverageReceipt::Current(feedback));
        assert_eq!(serde_json::to_value(restored).unwrap(), stored);
    }

    #[test]
    fn missing_geometry_is_incomplete_not_empty() {
        let mut comparison = comparison("", Some("a.rs"), Some("a.rs"));
        comparison.diffs.clear();
        let ledger = CoverageLedger::new(&comparison);
        assert!(!ledger.inventory.complete);
        assert_eq!(ledger.summary(false).percent, None);
    }
}
