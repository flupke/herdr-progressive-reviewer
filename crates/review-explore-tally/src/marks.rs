//! The review marks of the reviewer's snapshot, file by file, read from the tracker again only
//! for the files whose saved mark changed.

use std::collections::HashMap;

use review_explore::{ExploreRound, InterviewUpdate};
use review_repository::repository::{ChangedFile, RepoPath, Snapshot, SnapshotIdentity};
use review_state::{FileLines, ReviewTracker};
use review_store::{LoadResult, ReviewStore};
use review_types::MarkAuthor;

use crate::MarkTally;

/// One changed file and its review marks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FileMarks {
    pub(crate) file: ChangedFile,
    pub(crate) marks: Marks,
}

/// The review marks of one changed file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Marks {
    /// The file's changed lines, open or reviewed, with who reviewed each.
    Lines(FileLines),
    /// A file without lines to mark one by one, marked whole by an author or not at all.
    Whole(Option<MarkAuthor>),
    /// The marks could not be read: the file counts as left.
    Unread,
}

/// The review marks of the reviewer's latest snapshot, kept for the next reading: a file whose
/// saved mark did not change, in the same snapshot, is not read again.
#[derive(Debug)]
pub struct ChangeMarks {
    store: ReviewStore,
    snapshot: Option<SnapshotIdentity>,
    files: Vec<FileMarks>,
    /// The saved mark each file of `files` was read from.
    records: HashMap<RepoPath, LoadResult>,
}

impl ChangeMarks {
    /// Marks read from `store`, before any snapshot.
    pub fn new(store: ReviewStore) -> Self {
        Self {
            store,
            snapshot: None,
            files: Vec::new(),
            records: HashMap::new(),
        }
    }

    /// Reads the marks of `snapshot` through `tracker`.
    pub fn read(&mut self, tracker: &ReviewTracker, snapshot: &Snapshot) {
        if self.snapshot.as_ref() != Some(&snapshot.identity) {
            self.snapshot = Some(snapshot.identity.clone());
            self.files.clear();
            self.records.clear();
        }
        let mut previous: HashMap<RepoPath, FileMarks> = std::mem::take(&mut self.files)
            .into_iter()
            .map(|file| (file.file.review_path().clone(), file))
            .collect();
        let records = std::mem::take(&mut self.records);
        let files = snapshot
            .files
            .iter()
            .map(|file| {
                self.read_file(
                    tracker,
                    snapshot,
                    file,
                    previous.remove(file.review_path()),
                    &records,
                )
            })
            .collect();
        self.files = files;
    }

    /// The marks of `file`: `previous`, when they were read from the same saved mark.
    fn read_file(
        &mut self,
        tracker: &ReviewTracker,
        snapshot: &Snapshot,
        file: &ChangedFile,
        previous: Option<FileMarks>,
        records: &HashMap<RepoPath, LoadResult>,
    ) -> FileMarks {
        let path = file.review_path();
        let Ok(record) = self
            .store
            .load(snapshot.identity.review_unit(), path.as_bytes())
        else {
            return FileMarks {
                file: file.clone(),
                marks: Marks::Unread,
            };
        };
        let marks = previous
            .filter(|_| records.get(path) == Some(&record))
            .unwrap_or_else(|| FileMarks {
                file: file.clone(),
                marks: Marks::read(tracker, snapshot, file),
            });
        // Marks that could not be read are read again next time.
        if marks.marks != Marks::Unread {
            self.records.insert(path.clone(), record);
        }
        marks
    }

    /// The tally of the marks read last, in `round`, while the round waits for an answer to
    /// the question the agent's turn `question` posted. An answer marks nothing once the code
    /// changed since the round started: the question then adds nothing.
    pub fn mark_tally(
        &self,
        round: Option<&ExploreRound>,
        question: Option<&InterviewUpdate>,
    ) -> MarkTally {
        let current = |round: &ExploreRound| {
            self.snapshot
                .as_ref()
                .is_some_and(|snapshot| round.is_at(snapshot))
        };
        let question = question.filter(|_| round.is_some_and(current));
        MarkTally::of(&self.files, round, question)
    }
}

impl Marks {
    fn read(tracker: &ReviewTracker, snapshot: &Snapshot, file: &ChangedFile) -> Self {
        match tracker.lines(snapshot, file) {
            Ok(lines) if lines != FileLines::default() => Self::Lines(lines),
            // A file without lines to mark one by one is marked whole.
            Ok(_) => tracker
                .whole_file_author(snapshot, file)
                .map_or(Self::Unread, Self::Whole),
            Err(_) => Self::Unread,
        }
    }
}
