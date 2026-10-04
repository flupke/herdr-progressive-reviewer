//! One projection of the review threads onto the reviewed files.
//!
//! [`ThreadProjector`] rebuilds the projection from each file list and each
//! thread load. The files list and the Threads list read the threads, the file
//! each thread belongs to, per-file counts and placement from clones of one
//! [`SharedThreadProjection`] instead of each deriving them. Only the diff
//! loads the code a thread points at, so it reports each thread's
//! [`ThreadPlacement`] into the projection. Diff viewers still match threads
//! to the documents they show, which for Explore evidence are the files of a
//! saved comparison rather than the reviewed files.

use std::cell::{Ref, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use component_core::{Component, ComponentSubscriptions};
use review_threads::{ReviewThread, ReviewThreads, ThreadCounts, ThreadId, ThreadPaths};
use review_types::ReviewUnit;
use ui_events::{FileSummary, RepositoryFilesChanged, ReviewThreadsLoaded};

/// Where a thread's saved context sits in the code the diff shows now,
/// separate from the immutable saved context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThreadPlacement {
    /// The saved lines map to rows the diff shows.
    Current,
    /// The saved lines map to the file, but the diff hides their rows.
    Hidden,
    /// The saved lines no longer map to the file's current code.
    Earlier,
    /// The thread's file is not part of the reviewed diff.
    OutsideDiff,
    /// The file's code is not loaded, so only the saved context is known.
    Original,
    /// The thread's current source could not be read.
    Unavailable,
    /// The thread is the conversation of an Explore round, on no code.
    ExploreRound,
}

impl ThreadPlacement {
    pub fn label(self) -> &'static str {
        match self {
            Self::Current => "Current code",
            Self::Hidden => "Hidden code",
            Self::Earlier => "Earlier code",
            Self::OutsideDiff => "Outside diff",
            Self::Original => "Saved context",
            Self::Unavailable => "Unavailable",
            Self::ExploreRound => "Explore round",
        }
    }
}

/// The review's threads, the file each belongs to, per-file counts and
/// placement, for the review whose files were listed last.
#[derive(Default)]
pub struct ThreadProjection {
    review_unit: Option<ReviewUnit>,
    files: HashSet<String>,
    paths: ThreadPaths,
    threads: Option<ReviewThreads>,
    /// The threads before the latest load, so readers can tell what it changed.
    previous: Option<ReviewThreads>,
    file_counts: HashMap<String, ThreadCounts>,
    placements: HashMap<ThreadId, ThreadPlacement>,
}

impl ThreadProjection {
    /// The threads of the review, once loaded.
    pub fn threads(&self) -> Option<&ReviewThreads> {
        self.threads.as_ref()
    }

    /// The threads the latest load replaced.
    pub fn previous_threads(&self) -> Option<&ReviewThreads> {
        self.previous.as_ref()
    }

    /// The path a thread saved on `path` belongs to now: that reviewed
    /// file, or the single destination it was renamed to. Threads on paths
    /// that are not reviewed files keep their saved path.
    pub fn current_path<'a>(&'a self, path: &'a str) -> &'a str {
        self.paths.resolve(path)
    }

    /// Whether `thread` belongs to one of the reviewed files.
    fn is_on_reviewed_file(&self, thread: &ReviewThread) -> bool {
        thread
            .path()
            .is_some_and(|path| self.files.contains(self.current_path(path)))
    }

    /// Counts of the threads that belong to `path`.
    pub fn file_counts(&self, path: &str) -> ThreadCounts {
        self.file_counts.get(path).copied().unwrap_or_default()
    }

    /// Where the diff last placed `thread`. Before the diff reports it, a
    /// thread on a reviewed file shows its saved context and any other
    /// thread on code lies outside the diff.
    pub fn placement(&self, thread: &ReviewThread) -> ThreadPlacement {
        if thread.round().is_some() {
            return ThreadPlacement::ExploreRound;
        }
        self.placements
            .get(&thread.id)
            .copied()
            .unwrap_or(if self.is_on_reviewed_file(thread) {
                ThreadPlacement::Original
            } else {
                ThreadPlacement::OutsideDiff
            })
    }

    /// Replace the reviewed files; a new review forgets the previous one.
    fn list_files(&mut self, event: &RepositoryFilesChanged) {
        let review_unit = &event.review_checkpoint.review_unit;
        if self.review_unit.as_ref() != Some(review_unit) {
            *self = Self {
                review_unit: Some(review_unit.clone()),
                ..Self::default()
            };
        }
        self.files = event.files.iter().map(FileSummary::path).collect();
        self.paths = FileSummary::thread_paths(&event.files);
        self.count_files();
    }

    /// Accept threads loaded for the current review; a failed load keeps
    /// the threads already shown.
    fn load_threads(&mut self, event: &ReviewThreadsLoaded) {
        if self.review_unit.as_ref() != Some(&event.review_unit) {
            return;
        }
        if let Ok(threads) = &event.result {
            self.previous = self.threads.replace(threads.clone());
            self.count_files();
        }
    }

    fn place(&mut self, review_unit: &ReviewUnit, placements: HashMap<ThreadId, ThreadPlacement>) {
        if self.review_unit.as_ref() == Some(review_unit) {
            self.placements = placements;
        }
    }

    fn count_files(&mut self) {
        self.file_counts = self.threads.as_ref().map_or_else(HashMap::new, |threads| {
            threads
                .counts_by(|thread| {
                    thread
                        .path()
                        .map(|path| self.paths.resolve(path).to_owned())
                })
                .into_iter()
                .filter_map(|(path, counts)| Some((path?, counts)))
                .collect()
        });
    }
}

/// One projection shared by every component that shows review threads.
#[derive(Clone, Default)]
pub struct SharedThreadProjection(Rc<RefCell<ThreadProjection>>);

impl SharedThreadProjection {
    /// Borrow the current projection. Release it before the next event.
    ///
    /// # Panics
    ///
    /// Panics if called while the projection is being updated.
    pub fn read(&self) -> Ref<'_, ThreadProjection> {
        self.0.borrow()
    }

    /// Record where the diff placed each thread of `review_unit`.
    ///
    /// # Panics
    ///
    /// Panics if called while the projection is borrowed.
    pub fn place(&self, review_unit: &ReviewUnit, placements: HashMap<ThreadId, ThreadPlacement>) {
        self.0.borrow_mut().place(review_unit, placements);
    }
}

/// Rebuilds the shared projection from file lists and thread loads. Mount it
/// before the components that read the projection, so they see each update
/// while handling the same event.
pub struct ThreadProjector {
    projection: SharedThreadProjection,
}

impl ThreadProjector {
    pub fn new(projection: SharedThreadProjection) -> Self {
        Self { projection }
    }

    fn repository_changed(&mut self, event: &RepositoryFilesChanged) {
        self.projection.0.borrow_mut().list_files(event);
    }

    fn threads_loaded(&mut self, event: &ReviewThreadsLoaded) {
        self.projection.0.borrow_mut().load_threads(event);
    }
}

impl<A: Send + 'static> Component<A> for ThreadProjector {
    fn register_subscriptions(subscriptions: &mut ComponentSubscriptions<'_, Self, A>) {
        subscriptions.subscribe(Self::repository_changed);
        subscriptions.subscribe(Self::threads_loaded);
    }
}

#[cfg(test)]
#[path = "lib.tests.rs"]
mod tests;
