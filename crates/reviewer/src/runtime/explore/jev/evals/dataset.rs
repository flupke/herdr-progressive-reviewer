use std::{collections::BTreeSet, path::Path};

use eyre::{Result, ensure};
use review_explore::{CoverageUnit, SourceSide};
use review_repository::{
    diff::{DiffRow, parse_file_diff},
    repository::{ChangeKind, ChangedFile, DiffStatistics, FileKind, RepoPath},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Dataset {
    schema_version: u32,
    label_policy: String,
    cases: Vec<Case>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Case {
    pub(super) id: String,
    pub(super) path: String,
    pub(super) language: String,
    patch: String,
    origin: String,
    purpose: String,
    #[serde(default)]
    pub(super) context: serde_json::Value,
    pub(super) labels: Vec<LineLabel>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LineLabel {
    pub(super) side: SourceSide,
    pub(super) line: u32,
    /// 0: no independent review decision; 1: needs an explanation.
    pub(super) significance: u8,
    pub(super) category: String,
    pub(super) reason: String,
}

impl LineLabel {
    pub(super) fn unit(&self) -> CoverageUnit {
        CoverageUnit::Lines {
            file: 0,
            side: self.side,
            first: self.line,
            end: self.line + 1,
        }
    }
}

pub(super) struct Fixture {
    pub(super) case: Case,
    pub(super) patch: String,
    pub(super) file: ChangedFile,
    pub(super) hunks: Vec<Vec<DiffRow>>,
}

impl Dataset {
    pub(super) fn load(root: &Path) -> Result<(Vec<Fixture>, String)> {
        let bytes = std::fs::read(root.join("labels.json"))?;
        let dataset: Self = serde_json::from_slice(&bytes)?;
        ensure!(dataset.schema_version == 1, "unsupported dataset version");
        ensure!(!dataset.label_policy.is_empty(), "missing label policy");
        ensure!(!dataset.cases.is_empty(), "empty dataset");
        let mut hash = Sha256::new();
        hash.update(&bytes);
        let mut ids = BTreeSet::new();
        let mut fixtures = Vec::new();
        for case in dataset.cases {
            ensure!(ids.insert(case.id.clone()), "duplicate case {}", case.id);
            let fixture = Fixture::load(case, root)?;
            hash.update(fixture.patch.as_bytes());
            fixtures.push(fixture);
        }
        Ok((fixtures, format!("{:x}", hash.finalize())))
    }
}

impl Fixture {
    fn load(case: Case, root: &Path) -> Result<Self> {
        ensure!(
            Path::new(&case.patch)
                .components()
                .all(|part| matches!(part, std::path::Component::Normal(_))),
            "patch must be relative"
        );
        ensure!(
            !case.origin.is_empty() && !case.purpose.is_empty(),
            "case needs provenance and purpose"
        );
        let patch = std::fs::read_to_string(root.join(&case.patch))?;
        let source_path = RepoPath::from_bytes(case.path.as_bytes().to_vec());
        let file = ChangedFile {
            old_path: Some(source_path.clone()),
            new_path: Some(source_path),
            old_kind: FileKind::File,
            new_kind: FileKind::File,
            change: ChangeKind::Modified,
            display_path: case.path.clone(),
            statistics: DiffStatistics::from_unified_diff(patch.as_bytes()),
        };
        let rows = parse_file_diff(patch.as_bytes(), &file);
        ensure!(
            !rows.iter().any(|row| matches!(row, DiffRow::Notice { .. })),
            "{} has an invalid or unsupported diff",
            case.id
        );
        let hunks = Self::hunks(rows);
        let fixture = Self {
            case,
            patch,
            file,
            hunks,
        };
        fixture.validate_labels()?;
        Ok(fixture)
    }

    fn hunks(rows: Vec<DiffRow>) -> Vec<Vec<DiffRow>> {
        let mut hunks: Vec<Vec<DiffRow>> = Vec::new();
        for row in rows {
            if matches!(row, DiffRow::Hunk { .. }) {
                hunks.push(Vec::new());
            } else if let Some(hunk) = hunks.last_mut()
                && matches!(
                    row,
                    DiffRow::Add { .. } | DiffRow::Delete { .. } | DiffRow::Context { .. }
                )
            {
                hunk.push(row);
            }
        }
        hunks
    }

    pub(super) fn validate_labels(&self) -> Result<()> {
        let changed: BTreeSet<_> = self.hunks.iter().flatten().filter_map(coordinate).collect();
        let mut labelled = BTreeSet::new();
        for label in &self.case.labels {
            ensure!(
                label.significance <= 1 && label.line < u32::MAX,
                "invalid label"
            );
            ensure!(
                !label.reason.is_empty() && !label.category.is_empty(),
                "unexplained label"
            );
            ensure!(
                labelled.insert((label.side, label.line)),
                "duplicate line label"
            );
        }
        ensure!(!changed.is_empty(), "{} has no changed lines", self.case.id);
        ensure!(
            changed == labelled,
            "{} labels must match every old/new changed line exactly",
            self.case.id
        );
        Ok(())
    }
}

pub(super) fn coordinate(row: &DiffRow) -> Option<(SourceSide, u32)> {
    match row {
        DiffRow::Add { new_line, .. } => Some((SourceSide::New, *new_line)),
        DiffRow::Delete { old_line, .. } => Some((SourceSide::Old, *old_line)),
        _ => None,
    }
}
