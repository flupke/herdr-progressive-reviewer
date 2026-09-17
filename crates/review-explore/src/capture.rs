use std::borrow::Cow;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use crate::{CodeLocation, EvidenceRef, Source, SourceSide};
use review_guide::{FrozenFile, FrozenHunk, ReviewCheckpoint};
use review_repository::{
    diff::{DiffRow, parse_file_diff},
    repository::{ChangedFile, RepoPath, Repository, Snapshot, SnapshotIdentity},
};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManifestEntry {
    pub file: usize,
    /// None identifies file metadata/non-text changes; text hunks have one-based IDs.
    pub hunk: Option<usize>,
}

/// Internal comparison for native viewers; no repository catalog is sent to the agent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Comparison {
    pub repository_root: PathBuf,
    pub checkpoint: ReviewCheckpoint,
    pub files: Vec<ChangedFile>,
    pub context: Vec<FrozenFile>,
    pub diffs: Vec<Vec<u8>>,
    pub manifest: Vec<ManifestEntry>,
    pub sources: Vec<Source>,
    pub base: Option<SnapshotIdentity>,
}

impl Comparison {
    pub fn prepare(repository: &Repository, snapshot: &Snapshot) -> eyre::Result<Self> {
        let mut result = Self {
            repository_root: repository.root().to_owned(),
            checkpoint: ReviewCheckpoint::new(
                snapshot.identity.review_unit().clone(),
                snapshot.identity.snapshot_id(),
            ),
            files: snapshot.files.clone(),
            context: Vec::new(),
            diffs: Vec::new(),
            manifest: Vec::new(),
            sources: Vec::new(),
            base: Some(snapshot.identity.clone()),
        };
        for (index, file) in snapshot.files.iter().enumerate() {
            result.prepare_file(repository, snapshot, index, file)?;
        }
        Ok(result)
    }

    fn prepare_file(
        &mut self,
        repository: &Repository,
        snapshot: &Snapshot,
        index: usize,
        file: &ChangedFile,
    ) -> eyre::Result<()> {
        let diff = repository.diff(snapshot, file)?;
        let old_content = file
            .old_path
            .as_ref()
            .and_then(|path| self.base_source(path, repository.base_file_at(snapshot, path)));
        let new_content = file.new_path.as_ref().and_then(|path| {
            let mut source = Self::working_source(path);
            let content = match source.read(&self.repository_root) {
                Ok(content) => Some(content),
                Err(error) => {
                    source.limitation = Some(error.to_string());
                    None
                }
            };
            self.sources.push(source);
            content
        });
        let hunks: Vec<_> = parse_file_diff(&diff, file)
            .into_iter()
            .filter_map(|row| {
                if let DiffRow::Hunk {
                    old_start,
                    old_count,
                    new_start,
                    new_count,
                } = row
                {
                    Some(FrozenHunk {
                        old: (old_count > 0).then(|| {
                            old_start.saturating_sub(1)..old_start.saturating_sub(1) + old_count
                        }),
                        new: (new_count > 0).then(|| {
                            new_start.saturating_sub(1)..new_start.saturating_sub(1) + new_count
                        }),
                    })
                } else {
                    None
                }
            })
            .collect();
        self.manifest.push(ManifestEntry {
            file: index,
            hunk: None,
        });
        self.manifest
            .extend((1..=hunks.len()).map(|hunk| ManifestEntry {
                file: index,
                hunk: Some(hunk),
            }));
        self.context.push(FrozenFile {
            path: file.review_path().display(),
            hunk_count: hunks.len(),
            old_path: file.old_path.as_ref().map(RepoPath::display),
            new_path: file.new_path.as_ref().map(RepoPath::display),
            old_content,
            new_content,
            hunks,
            diff_hash: format!("{:x}", Sha256::digest(&diff)),
        });
        self.diffs.push(diff);
        Ok(())
    }

    fn working_source(path: &RepoPath) -> Source {
        Source {
            path: path.clone(),
            display_path: path.display(),
            side: SourceSide::New,
            content: None,
            limitation: None,
            base: None,
        }
    }

    fn base_source(
        &mut self,
        path: &RepoPath,
        result: review_repository::Result<Vec<u8>>,
    ) -> Option<Vec<u8>> {
        let (content, limitation) = match result {
            Ok(content) => (Some(content), None),
            Err(error) => (None, Some(error.to_string())),
        };
        self.sources.push(Source {
            path: path.clone(),
            display_path: path.display(),
            side: SourceSide::Old,
            content: content.clone(),
            limitation,
            base: None,
        });
        content
    }

    pub fn source(&self, location: &CodeLocation) -> Option<Cow<'_, Source>> {
        location.relative_path()?;
        if let Some(source) = self
            .sources
            .iter()
            .find(|source| source.side == location.side && source.path == location.path)
        {
            return Some(Cow::Borrowed(source));
        }
        let mut source = Self::working_source(&location.path);
        source.side = location.side;
        source.base.clone_from(&self.base);
        Some(Cow::Owned(source))
    }

    pub fn working_source_at(&self, relative: &Path) -> Option<Source> {
        self.source(&CodeLocation {
            path: RepoPath::from_bytes(relative.as_os_str().as_bytes()),
            side: SourceSide::New,
            lines: None,
        })
        .map(Cow::into_owned)
    }

    pub(crate) fn validate_location(&self, location: &CodeLocation) -> bool {
        let Some(source) = self.source(location) else {
            return false;
        };
        if source.side == SourceSide::New && source.disk_path(&self.repository_root).is_err() {
            return false;
        }
        if source.read(&self.repository_root).is_err() {
            return location.lines.is_none() && source.limitation.is_some();
        }
        location.lines.as_ref().is_none_or(|range| {
            source.read_text(&self.repository_root).is_ok_and(|text| {
                range.first_line > 0
                    && range.last_line >= range.first_line
                    && range.last_line as usize <= text.lines().count()
            })
        })
    }

    pub(crate) fn validate_evidence(&self, evidence: &EvidenceRef) -> bool {
        !evidence.relationship.trim().is_empty() && self.validate_location(&evidence.location)
    }

    pub(crate) fn maps(&self, entry: &ManifestEntry, location: &CodeLocation) -> bool {
        let file = &self.files[entry.file];
        let path = match location.side {
            SourceSide::Old => &file.old_path,
            SourceSide::New => &file.new_path,
        };
        if path.as_ref() != Some(&location.path) {
            return false;
        }
        let (Some(hunk), Some(lines)) = (entry.hunk, &location.lines) else {
            return true;
        };
        let hunk = &self.context[entry.file].hunks[hunk - 1];
        let range = match location.side {
            SourceSide::Old => &hunk.old,
            SourceSide::New => &hunk.new,
        };
        range
            .as_ref()
            .is_some_and(|range| lines.first_line <= range.end && lines.last_line > range.start)
    }
}
