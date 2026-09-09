//! Bounded reuse of immutable diff data, keyed by snapshot and saved review mark.

use std::collections::HashMap;

use review_repository::repository::{RepoPath, SnapshotIdentity};
use review_store::LoadResult;

use super::ReviewDiff;

const MAX_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Default)]
pub(super) struct DiffCache {
    snapshot: Option<SnapshotIdentity>,
    files: HashMap<RepoPath, CachedDiff>,
    bytes: usize,
}

#[derive(Debug)]
struct CachedDiff {
    record: LoadResult,
    diff: ReviewDiff,
}

impl DiffCache {
    pub(super) fn get(
        &mut self,
        snapshot: &SnapshotIdentity,
        path: &RepoPath,
        record: &LoadResult,
    ) -> Option<ReviewDiff> {
        if self.snapshot.as_ref() != Some(snapshot) {
            self.files.clear();
            self.bytes = 0;
            self.snapshot = Some(snapshot.clone());
        }
        self.files
            .get(path)
            .filter(|cached| &cached.record == record)
            .map(|cached| cached.diff.clone())
    }

    pub(super) fn insert(
        &mut self,
        snapshot: &SnapshotIdentity,
        path: RepoPath,
        record: LoadResult,
        diff: &ReviewDiff,
    ) {
        // A slow read from an older snapshot must not replace newer cached data.
        if self.snapshot.as_ref() != Some(snapshot) {
            return;
        }
        if let Some(previous) = self.files.remove(&path) {
            self.bytes -= previous.diff.byte_len();
        }
        let bytes = diff.byte_len();
        if bytes > MAX_BYTES {
            return;
        }
        if self.bytes + bytes > MAX_BYTES {
            self.files.clear();
            self.bytes = 0;
        }
        self.bytes += bytes;
        self.files.insert(
            path,
            CachedDiff {
                record,
                diff: diff.clone(),
            },
        );
    }
}

impl ReviewDiff {
    fn byte_len(&self) -> usize {
        self.unified.len()
            + self.old_content.as_ref().map_or(0, Vec::len)
            + self.new_content.as_ref().map_or(0, Vec::len)
    }
}
