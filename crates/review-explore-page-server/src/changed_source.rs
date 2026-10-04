//! The change a data set's citations name: a few files, each with its text before and after the
//! change and its diff, as the review tool reads them from the repository.

use review_explore::{CitedLines, CodeLocation, EvidenceRef, SourceSide, Uncitable};
use review_explore_citations::{Citation, CodeColors};
use review_repository::diff::parse_file_diff;
use review_repository::repository::{ChangeKind, ChangedFile, DiffStatistics, FileKind, RepoPath};
use review_source::SourceLineRange;

/// A file the change modifies.
pub(crate) struct ChangedSource {
    pub(crate) path: &'static str,
    pub(crate) old: &'static str,
    pub(crate) new: &'static str,
    /// The file's diff, as `git diff` writes it.
    pub(crate) diff: &'static str,
}

impl ChangedSource {
    /// A citation of this file on `side`: `lines`, or the whole file.
    pub(crate) fn evidence(
        &self,
        side: SourceSide,
        lines: Option<(u32, u32)>,
        notes: &str,
    ) -> EvidenceRef {
        EvidenceRef {
            location: CodeLocation {
                path: RepoPath::from_bytes(self.path),
                side,
                lines: lines.map(|(first_line, last_line)| SourceLineRange {
                    first_line,
                    last_line,
                }),
            },
            notes: notes.into(),
        }
    }

    /// The lines `location` cites in this file's diff.
    fn cited_lines(&self, location: &CodeLocation) -> Result<CitedLines, Uncitable> {
        CitedLines::in_diff(
            location,
            &parse_file_diff(self.diff.as_bytes(), &self.changed_file()),
            Some(self.old.into()),
            Some(self.new.into()),
        )
    }

    fn changed_file(&self) -> ChangedFile {
        let rows = || {
            self.diff
                .lines()
                .filter(|line| !line.starts_with("+++") && !line.starts_with("---"))
        };
        ChangedFile {
            old_path: Some(RepoPath::from_bytes(self.path)),
            new_path: Some(RepoPath::from_bytes(self.path)),
            old_kind: FileKind::File,
            new_kind: FileKind::File,
            change: ChangeKind::Modified,
            display_path: self.path.into(),
            statistics: DiffStatistics {
                lines_added: rows().filter(|line| line.starts_with('+')).count() as u64,
                lines_removed: rows().filter(|line| line.starts_with('-')).count() as u64,
            },
        }
    }
}

/// The files of a change. Every other path stands for a file outside the change and the
/// repository's tracked files, such as an ignored `.env`.
pub(crate) struct FixedChange(pub(crate) &'static [ChangedSource]);

impl FixedChange {
    /// `evidence` with the lines it cites in the change, or none for a file outside it.
    pub(crate) fn cite(&self, evidence: &EvidenceRef) -> Citation {
        let path = &evidence.location.path;
        let lines = self
            .0
            .iter()
            .find(|source| *path == RepoPath::from_bytes(source.path))
            .map_or(Err(Uncitable::Untracked), |source| {
                source.cited_lines(&evidence.location)
            });
        CodeColors::default().cite(evidence.clone(), lines)
    }
}
