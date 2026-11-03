//! The files on which the review marks of a round's turns still name the lines the round
//! read, once the code may have changed since the round started.

use std::collections::BTreeSet;

use review_repository::repository::{ChangedFile, RepoPath, Repository, Snapshot};

use crate::Comparison;

/// The changed files of a snapshot on which a round's marks apply: the lines a turn names are
/// the same lines at the snapshot as at the round's checkpoint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MarkableFiles {
    /// The snapshot is the round's checkpoint: every file.
    All,
    /// The code changed since the round started: the files whose diff is still the one the
    /// round captured (Git's abbreviated object names aside), so that their changed lines keep
    /// their numbers on both sides. A file of another review, or one the round never captured,
    /// is not one of them.
    Unchanged {
        unchanged: BTreeSet<RepoPath>,
        /// The files whose diff could not be read: not marked now, and not known to differ.
        unread: BTreeSet<RepoPath>,
    },
}

impl MarkableFiles {
    /// No file.
    pub fn none() -> Self {
        Self::Unchanged {
            unchanged: BTreeSet::new(),
            unread: BTreeSet::new(),
        }
    }

    /// The files of `snapshot` on which the marks of the round that captured `comparison`
    /// apply.
    pub fn at(comparison: &Comparison, repository: &Repository, snapshot: &Snapshot) -> Self {
        let identity = &snapshot.identity;
        let checkpoint = &comparison.checkpoint;
        if checkpoint.matches(identity.review_unit(), identity.snapshot_id()) {
            return Self::All;
        }
        let mut unchanged = BTreeSet::new();
        let mut unread = BTreeSet::new();
        if identity.review_unit() != &checkpoint.review_unit {
            return Self::none();
        }
        for file in &snapshot.files {
            let Some(captured) = comparison.captured_diff(file) else {
                continue;
            };
            match repository.diff(snapshot, file) {
                Ok(current) if same_diff(&current, captured) => {
                    unchanged.insert(file.review_path().clone());
                }
                Ok(_) => {}
                Err(_) => {
                    unread.insert(file.review_path().clone());
                }
            }
        }
        Self::Unchanged { unchanged, unread }
    }

    /// Whether marks apply on `file`.
    pub fn allows(&self, file: &ChangedFile) -> bool {
        match self {
            Self::All => true,
            Self::Unchanged { unchanged, .. } => unchanged.contains(file.review_path()),
        }
    }

    /// Whether the diff of `file` could not be read, so that nothing tells whether it changed.
    pub fn unread(&self, file: &ChangedFile) -> bool {
        match self {
            Self::All => false,
            Self::Unchanged { unread, .. } => unread.contains(file.review_path()),
        }
    }

    /// These files of `snapshot` less `diverged`, the files whose diff had changed when the
    /// agent wrote its turn: its line numbers there may name other lines, even once the code
    /// is back to the round's.
    #[must_use]
    pub fn excluding(self, snapshot: &Snapshot, diverged: &[String]) -> Self {
        if diverged.is_empty() {
            return self;
        }
        let unchanged = snapshot
            .files
            .iter()
            .filter(|file| self.allows(file))
            .filter(|file| !diverged.contains(&file.review_path().display()))
            .map(|file| file.review_path().clone())
            .collect();
        let unread = match self {
            Self::All => BTreeSet::new(),
            Self::Unchanged { unread, .. } => unread,
        };
        Self::Unchanged { unchanged, unread }
    }

    /// The files of the round that captured `comparison` whose diff differs at `snapshot`, by
    /// their review paths: those whose diff changed, and those `snapshot` no longer changes.
    /// A file whose diff could not be read is not one of them.
    pub fn diverged(&self, comparison: &Comparison, snapshot: &Snapshot) -> Vec<String> {
        if *self == Self::All {
            return Vec::new();
        }
        comparison
            .files
            .iter()
            .filter(|captured| {
                match snapshot.files.iter().find(|file| {
                    file.old_path == captured.old_path && file.new_path == captured.new_path
                }) {
                    Some(file) => !self.allows(file) && !self.unread(file),
                    None => true,
                }
            })
            .map(|captured| captured.review_path().display())
            .collect()
    }

    /// Whether the result is complete: every diff was read.
    pub fn complete(&self) -> bool {
        match self {
            Self::All => true,
            Self::Unchanged { unread, .. } => unread.is_empty(),
        }
    }
}

/// Whether two diffs of a file are the same: byte for byte, but for the abbreviated object
/// names of a Git `index` line, whose length grows with the repository.
fn same_diff(current: &[u8], captured: &[u8]) -> bool {
    let lines = |diff: &[u8]| {
        diff.split(|byte| *byte == b'\n')
            .map(<[u8]>::to_vec)
            .collect::<Vec<_>>()
    };
    let (current, captured) = (lines(current), lines(captured));
    current.len() == captured.len()
        && current
            .iter()
            .zip(&captured)
            .all(|(current, captured)| current == captured || same_index(current, captured))
}

/// Whether two `index <old>..<new> [<mode>]` lines name the same objects, one abbreviation a
/// prefix of the other.
fn same_index(current: &[u8], captured: &[u8]) -> bool {
    let parts = |line: &[u8]| -> Option<(Vec<u8>, Vec<u8>, Vec<u8>)> {
        let rest = line.strip_prefix(b"index ")?;
        let (objects, mode) = match rest.iter().position(|byte| *byte == b' ') {
            Some(space) => (&rest[..space], &rest[space..]),
            None => (rest, &[][..]),
        };
        let dots = objects.windows(2).position(|pair| pair == b"..")?;
        Some((
            objects[..dots].to_vec(),
            objects[dots + 2..].to_vec(),
            mode.to_vec(),
        ))
    };
    let prefixes = |a: &[u8], b: &[u8]| {
        !a.is_empty() && !b.is_empty() && (a.starts_with(b) || b.starts_with(a))
    };
    match (parts(current), parts(captured)) {
        (Some((old, new, mode)), Some((old_then, new_then, mode_then))) => {
            prefixes(&old, &old_then) && prefixes(&new, &new_then) && mode == mode_then
        }
        _ => false,
    }
}

impl Comparison {
    /// The diff the round captured of `file`, when it captured the same paths.
    fn captured_diff(&self, file: &ChangedFile) -> Option<&[u8]> {
        let index = self.files.iter().position(|captured| {
            captured.old_path == file.old_path && captured.new_path == file.new_path
        })?;
        self.diffs.get(index).map(Vec::as_slice)
    }
}

#[cfg(test)]
#[path = "markable.tests.rs"]
mod tests;
