use super::*;
use component_core::ComponentEventBus;
use review_repository::repository::{ChangeKind, ChangedFile};
use review_source::{AnchorKind, DiffRangeAnchor, ReviewCheckpoint};
use review_state::ReviewStatus;
use review_threads::{Post, Resolution, SavedDrafts};

struct Review {
    bus: ComponentEventBus<()>,
    projection: SharedThreadProjection,
    threads: ReviewThreads,
}

impl Review {
    fn new() -> Self {
        let projection = SharedThreadProjection::default();
        let mut bus = ComponentEventBus::new();
        bus.mount(|_| ThreadProjector::new(projection.clone()));
        Self {
            bus,
            projection,
            threads: ReviewThreads::new("change".into()),
        }
    }

    fn list(&mut self, review_unit: &str, files: Vec<FileSummary>) {
        self.bus
            .publish(RepositoryFilesChanged {
                review_checkpoint: ReviewCheckpoint::new(review_unit, "now"),
                files,
            })
            .unwrap();
    }

    /// Start a thread on `path` and load the review's threads.
    fn start(&mut self, path: &str) -> ThreadId {
        let post = Post::start(anchor(path), String::new(), "Why?".into());
        let id = post.thread_id().clone();
        self.threads.post(post).unwrap();
        self.load(Ok(self.threads.clone()));
        id
    }

    fn load(&mut self, result: Result<ReviewThreads, String>) {
        self.bus
            .publish(ReviewThreadsLoaded {
                review_unit: "change".into(),
                result,
                drafts: SavedDrafts::default(),
            })
            .unwrap();
    }

    fn thread(&self, id: &ThreadId) -> ReviewThread {
        self.threads.thread(id).unwrap().clone()
    }
}

fn anchor(path: &str) -> DiffRangeAnchor {
    DiffRangeAnchor {
        source_checkpoint: "original".into(),
        old_path: None,
        new_path: Some(path.into()),
        old_lines: None,
        new_lines: Some(0..1),
        target_kind: AnchorKind::Lines,
        source_hunk_count: 1,
        old_content: None,
        new_content: Some(b"line\n".to_vec()),
        diff_hash: String::new(),
    }
}

fn modified(path: &str) -> FileSummary {
    FileSummary::new(path, ReviewStatus::Unreviewed)
}

fn renamed(from: &str, to: &str) -> FileSummary {
    let mut file = ChangedFile::modified(to);
    file.change = ChangeKind::Renamed;
    file.old_path = ChangedFile::modified(from).old_path;
    FileSummary::from_changed(&file, ReviewStatus::Unreviewed)
}

#[test]
fn a_thread_on_a_renamed_file_belongs_to_its_new_path() {
    let mut review = Review::new();
    review.list("change", vec![renamed("src/old.rs", "src/new.rs")]);
    let id = review.start("src/old.rs");
    let projection = review.projection.read();
    let thread = review.thread(&id);
    assert_eq!(projection.current_path(thread.path()), "src/new.rs");
    assert!(projection.is_on_reviewed_file(&thread));
    assert_eq!(projection.file_counts("src/new.rs").open, 1);
    assert_eq!(projection.file_counts("src/old.rs").total, 0);
    assert_eq!(projection.placement(&thread), ThreadPlacement::Original);
}

#[test]
fn a_thread_on_a_copied_file_stays_on_the_source() {
    let mut review = Review::new();
    review.list(
        "change",
        vec![
            modified("src/source.rs"),
            renamed("src/source.rs", "src/copy.rs"),
        ],
    );
    let id = review.start("src/source.rs");
    let projection = review.projection.read();
    let thread = review.thread(&id);
    assert_eq!(projection.current_path(thread.path()), "src/source.rs");
    assert_eq!(projection.file_counts("src/source.rs").total, 1);
    assert_eq!(projection.file_counts("src/copy.rs").total, 0);
    assert_eq!(projection.placement(&thread), ThreadPlacement::Original);
}

#[test]
fn a_thread_whose_source_was_copied_twice_keeps_its_saved_path() {
    let mut review = Review::new();
    review.list(
        "change",
        vec![
            renamed("src/source.rs", "src/first.rs"),
            renamed("src/source.rs", "src/second.rs"),
        ],
    );
    let id = review.start("src/source.rs");
    let projection = review.projection.read();
    let thread = review.thread(&id);
    assert_eq!(projection.current_path(thread.path()), "src/source.rs");
    assert!(!projection.is_on_reviewed_file(&thread));
    assert_eq!(projection.file_counts("src/source.rs").total, 1);
    assert_eq!(projection.placement(&thread), ThreadPlacement::OutsideDiff);
}

#[test]
fn a_thread_on_a_missing_file_lies_outside_the_diff() {
    let mut review = Review::new();
    review.list("change", vec![modified("src/lib.rs")]);
    let id = review.start("gone.rs");
    let projection = review.projection.read();
    let thread = review.thread(&id);
    assert_eq!(projection.current_path(thread.path()), "gone.rs");
    assert!(!projection.is_on_reviewed_file(&thread));
    assert_eq!(projection.file_counts("gone.rs").total, 1);
    assert_eq!(projection.file_counts("src/lib.rs").total, 0);
    assert_eq!(projection.placement(&thread), ThreadPlacement::OutsideDiff);
}

#[test]
fn counts_follow_each_load_and_a_failed_load_keeps_the_threads() {
    let mut review = Review::new();
    review.list("change", vec![modified("src/lib.rs")]);
    let first = review.start("src/lib.rs");
    review.start("src/lib.rs");
    review
        .threads
        .set_resolution(&first, Resolution::Resolved)
        .unwrap();
    review.load(Ok(review.threads.clone()));
    review.load(Err("disk full".into()));
    let projection = review.projection.read();
    let counts = projection.file_counts("src/lib.rs");
    assert_eq!((counts.total, counts.open), (2, 1));
    assert_eq!(
        projection
            .previous_threads()
            .and_then(|threads| threads.thread(&first))
            .map(|thread| thread.resolution),
        Some(Resolution::Open)
    );
}

#[test]
fn the_diff_placement_replaces_the_default_for_the_current_review_only() {
    let mut review = Review::new();
    review.list("change", vec![modified("src/lib.rs")]);
    let id = review.start("src/lib.rs");
    let thread = review.thread(&id);
    review.projection.place(
        &"other".into(),
        HashMap::from([(id.clone(), ThreadPlacement::Hidden)]),
    );
    assert_eq!(
        review.projection.read().placement(&thread),
        ThreadPlacement::Original
    );
    review.projection.place(
        &"change".into(),
        HashMap::from([(id.clone(), ThreadPlacement::Current)]),
    );
    assert_eq!(
        review.projection.read().placement(&thread),
        ThreadPlacement::Current
    );
    review.list(
        "change",
        vec![modified("src/lib.rs"), modified("src/new.rs")],
    );
    assert_eq!(
        review.projection.read().placement(&thread),
        ThreadPlacement::Current
    );
}

#[test]
fn another_review_forgets_threads_and_placement_and_ignores_stale_loads() {
    let mut review = Review::new();
    review.list("change", vec![modified("src/lib.rs")]);
    let id = review.start("src/lib.rs");
    review.projection.place(
        &"change".into(),
        HashMap::from([(id.clone(), ThreadPlacement::Current)]),
    );
    review.list("other", vec![modified("src/lib.rs")]);
    review.load(Ok(review.threads.clone()));
    let projection = review.projection.read();
    assert!(projection.threads().is_none());
    assert_eq!(projection.file_counts("src/lib.rs").total, 0);
    assert_eq!(
        projection.placement(&review.thread(&id)),
        ThreadPlacement::Original
    );
}
