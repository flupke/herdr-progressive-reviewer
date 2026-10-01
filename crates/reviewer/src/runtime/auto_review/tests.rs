use super::*;
use review_explore::{CoverageUnit, SignificanceResult};
use review_repository::repository::RepoType;
use review_test_support::{
    ReviewRepositoryFixture, complete_repository_snapshot, repository_fixture,
};

struct Fixture {
    files: Box<dyn ReviewRepositoryFixture>,
    _state: tempfile::TempDir,
    repository: Repository,
    store: ReviewStore,
    tracker: ReviewTracker,
}

impl Fixture {
    fn new(kind: RepoType) -> Self {
        let files = repository_fixture(kind);
        let state = tempfile::tempdir().unwrap();
        let repository = Repository::discover(files.root())
            .unwrap()
            .with_state_root(state.path());
        let store = ReviewStore::open(state.path(), repository.root()).unwrap();
        let tracker = ReviewTracker::new(repository.clone(), store.clone());
        Self {
            files,
            _state: state,
            repository,
            store,
            tracker,
        }
    }

    fn prepare(&self) -> AutoReview {
        let snapshot = complete_repository_snapshot(&self.repository);
        let checkpoint = ReviewCheckpoint::new(
            snapshot.identity.review_unit().clone(),
            snapshot.identity.snapshot_id(),
        );
        AutoReview::prepare(&self.repository, &self.tracker, &self.store, &checkpoint).unwrap()
    }

    fn record(&self, review: &AutoReview, path: &str) -> LoadResult {
        self.store
            .load(&review.comparison.checkpoint.review_unit, path.as_bytes())
            .unwrap()
    }
}

impl AutoReview {
    fn classify_file(&mut self, path: &str, outcome: Significance) {
        let file = self
            .comparison
            .files
            .iter()
            .position(|file| file.review_path().display() == path)
            .unwrap();
        let units = self
            .coverage
            .inventory()
            .units
            .iter()
            .filter(|unit| unit.file_index() == file && matches!(unit, CoverageUnit::Lines { .. }))
            .cloned()
            .collect();
        self.record_classification(path, units, outcome);
    }

    fn record_classification(&mut self, id: &str, units: Vec<CoverageUnit>, outcome: Significance) {
        assert!(self.coverage.record_significance(SignificanceResult {
            id: id.into(),
            units,
            outcome,
            model: Some("jev-1.13.0".into()),
            rubric: super::super::jev::RUBRIC.into(),
            criterion: String::new(),
            input_references: vec![],
            omissions: vec![],
            probabilities: std::collections::BTreeMap::default(),
            confidence: Some(0.99),
            error: None,
        }));
        self.finished = true;
    }
}

#[test_case::test_case(RepoType::Git; "git")]
#[test_case::test_case(RepoType::Jj; "jj")]
fn only_fully_insignificant_files_are_marked_and_later_edits_need_review(kind: RepoType) {
    let fixture = Fixture::new(kind);
    for path in [
        "docs.md",
        "code.rs",
        "uncertain.rs",
        "failed.rs",
        "large.rs",
        "missing.rs",
        "partial.rs",
    ] {
        fixture.files.write(path, b"first\nsecond\n");
    }
    fixture.files.write("binary", b"\0\xff");
    fixture.files.write("empty", b"");
    std::os::unix::fs::symlink("docs.md", fixture.files.root().join("symlink")).unwrap();
    let mut review = fixture.prepare();
    assert!(
        !review.coverage.inventory().complete,
        "symlinks have no supported text inventory"
    );
    for (path, outcome) in [
        ("docs.md", Significance::Insignificant),
        ("code.rs", Significance::Significant),
        ("uncertain.rs", Significance::Uncertain),
        ("failed.rs", Significance::Failed),
        ("large.rs", Significance::Oversized),
    ] {
        review.classify_file(path, outcome);
    }
    let partial = review
        .comparison
        .files
        .iter()
        .position(|file| file.review_path().display() == "partial.rs")
        .unwrap();
    review.record_classification(
        "partial",
        vec![CoverageUnit::Lines {
            file: partial,
            side: review_explore::SourceSide::New,
            first: 1,
            end: 2,
        }],
        Significance::Insignificant,
    );
    let summary = review
        .apply(&fixture.repository, &fixture.tracker, &fixture.store)
        .unwrap();
    assert_eq!(summary.marked, 1);
    assert_eq!(summary.failed_classifications, 2);
    for file in &review.comparison.files {
        assert_eq!(
            matches!(
                fixture.record(&review, &file.review_path().display()),
                LoadResult::Reviewed(_)
            ),
            file.review_path().display() == "docs.md"
        );
    }
    let baseline = review.comparison.checkpoint.checkpoint.clone();
    let LoadResult::Reviewed(record) = fixture.record(&review, "docs.md") else {
        panic!("expected a reviewed file")
    };
    assert_eq!(record.baseline_commit_id, baseline);
    fixture
        .files
        .write("docs.md", b"changed after automatic review\n");
    let snapshot = complete_repository_snapshot(&fixture.repository);
    let file = snapshot
        .files
        .iter()
        .find(|file| file.review_path().display() == "docs.md")
        .unwrap();
    assert_eq!(
        fixture.tracker.status(&snapshot, file).unwrap().status,
        review_state::ReviewStatus::ChangedSinceReview
    );
}

#[test_case::test_case(RepoType::Git; "git")]
#[test_case::test_case(RepoType::Jj; "jj")]
fn stale_classification_cannot_mark_new_edits_or_another_review(kind: RepoType) {
    let fixture = Fixture::new(kind);
    fixture.files.write("docs.md", b"initial docs\n");
    let mut review = fixture.prepare();
    review.classify_file("docs.md", Significance::Insignificant);
    fixture.files.write("docs.md", b"unclassified changes\n");
    assert!(
        review
            .apply(&fixture.repository, &fixture.tracker, &fixture.store)
            .is_err()
    );
    assert_eq!(fixture.record(&review, "docs.md"), LoadResult::Unreviewed);
    let mut review = fixture.prepare();
    review.classify_file("docs.md", Significance::Insignificant);
    fixture.files.new_change("switch review");
    assert!(
        review
            .apply(&fixture.repository, &fixture.tracker, &fixture.store)
            .is_err()
    );
    assert_eq!(fixture.record(&review, "docs.md"), LoadResult::Unreviewed);
}

#[test]
fn cancellation_and_manual_marks_take_precedence_over_automatic_results() {
    let fixture = Fixture::new(RepoType::Git);
    fixture.files.write("docs.md", b"initial docs\n");
    let mut review = fixture.prepare();
    review.classify_file("docs.md", Significance::Insignificant);
    review.active.store(false, Ordering::Relaxed);
    assert!(
        review
            .apply(&fixture.repository, &fixture.tracker, &fixture.store)
            .is_err()
    );
    assert_eq!(fixture.record(&review, "docs.md"), LoadResult::Unreviewed);
    review.active.store(true, Ordering::Relaxed);
    fixture
        .store
        .mark(
            &review.comparison.checkpoint.review_unit,
            b"docs.md",
            "aabb0011",
        )
        .unwrap();
    let prior = fixture.record(&review, "docs.md");
    assert_eq!(
        review
            .apply(&fixture.repository, &fixture.tracker, &fixture.store)
            .unwrap()
            .marked,
        0
    );
    assert_eq!(fixture.record(&review, "docs.md"), prior);
}

#[test]
fn metadata_changes_remain_unreviewed_even_when_all_text_is_insignificant() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new(RepoType::Git);
    fixture
        .files
        .write("script.sh", b"#!/bin/sh\n# old comment\n");
    fixture.files.new_change("base script");
    fixture
        .files
        .write("script.sh", b"#!/bin/sh\n# new comment\n");
    std::fs::set_permissions(
        fixture.files.root().join("script.sh"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let mut review = fixture.prepare();
    review.classify_file("script.sh", Significance::Insignificant);
    assert!(
        review
            .coverage
            .inventory()
            .units
            .iter()
            .any(|unit| matches!(unit, CoverageUnit::Item { name, .. } if name == "mode"))
    );
    assert_eq!(
        review
            .apply(&fixture.repository, &fixture.tracker, &fixture.store)
            .unwrap()
            .marked,
        0
    );
    assert_eq!(fixture.record(&review, "script.sh"), LoadResult::Unreviewed);
}

/// Twenty numbered lines with some of them rewritten.
fn numbered(edits: &[(u32, &str)]) -> Vec<u8> {
    (1..=20)
        .map(|line| {
            edits
                .iter()
                .find(|(edited, _)| *edited == line)
                .map_or_else(|| format!("{line}\n"), |(_, text)| format!("{text}\n"))
        })
        .collect::<String>()
        .into_bytes()
}

impl AutoReview {
    /// Classify one changed line on both sides as insignificant.
    fn classify_line(&mut self, path: &str, line: u32) {
        let file = self
            .comparison
            .files
            .iter()
            .position(|file| file.review_path().display() == path)
            .unwrap();
        let units = [
            review_explore::SourceSide::Old,
            review_explore::SourceSide::New,
        ]
        .into_iter()
        .map(|side| CoverageUnit::Lines {
            file,
            side,
            first: line,
            end: line + 1,
        })
        .collect();
        self.record_classification(
            &format!("{path}:{line}"),
            units,
            Significance::Insignificant,
        );
    }
}

#[test_case::test_case(RepoType::Git; "git")]
#[test_case::test_case(RepoType::Jj; "jj")]
fn insignificant_hunks_of_other_files_are_marked_reviewed(kind: RepoType) {
    let fixture = Fixture::new(kind);
    fixture.files.write("mixed.rs", &numbered(&[]));
    fixture.files.new_change("review");
    fixture
        .files
        .write("mixed.rs", &numbered(&[(2, "two"), (15, "fifteen")]));
    let mut review = fixture.prepare();
    review.classify_line("mixed.rs", 2);

    let summary = review
        .apply(&fixture.repository, &fixture.tracker, &fixture.store)
        .unwrap();

    assert_eq!((summary.marked, summary.hunks), (0, 1));
    let snapshot = complete_repository_snapshot(&fixture.repository);
    let diff = fixture.tracker.diff(&snapshot, &snapshot.files[0]).unwrap();
    assert_eq!(
        diff.hunks
            .reviewed
            .iter()
            .map(|hunk| (hunk.span.new.start, hunk.span.new.end))
            .collect::<Vec<_>>(),
        [(1, 2)]
    );
    assert_eq!(
        diff.hunks
            .open
            .iter()
            .map(|hunk| (hunk.span.new.start, hunk.span.new.end))
            .collect::<Vec<_>>(),
        [(14, 15)]
    );
}

impl Fixture {
    /// Accept the open hunk on one zero-based current line by hand.
    fn accept_hunk_at(&self, line: u32) {
        let snapshot = complete_repository_snapshot(&self.repository);
        let file = &snapshot.files[0];
        let span = self
            .tracker
            .diff(&snapshot, file)
            .unwrap()
            .hunks
            .open
            .into_iter()
            .find(|hunk| hunk.span.new.contains(&line))
            .unwrap()
            .span;
        self.tracker
            .mark_hunk(&snapshot, file, &review_hunks::HunkMark::Review(span))
            .unwrap();
    }

    fn status(&self) -> review_state::ReviewStatus {
        let snapshot = complete_repository_snapshot(&self.repository);
        self.tracker
            .status(&snapshot, &snapshot.files[0])
            .unwrap()
            .status
    }
}

#[test_case::test_case(RepoType::Git; "git")]
#[test_case::test_case(RepoType::Jj; "jj")]
fn a_partly_reviewed_file_whose_other_hunks_are_insignificant_is_reviewed(kind: RepoType) {
    let fixture = Fixture::new(kind);
    fixture.files.write("mixed.rs", &numbered(&[]));
    fixture.files.new_change("review");
    fixture
        .files
        .write("mixed.rs", &numbered(&[(2, "two"), (15, "fifteen")]));
    fixture.accept_hunk_at(14);
    let mut review = fixture.prepare();
    review.classify_line("mixed.rs", 2);

    let summary = review
        .apply(&fixture.repository, &fixture.tracker, &fixture.store)
        .unwrap();

    assert_eq!(summary.hunks, 1);
    assert_eq!(fixture.status(), review_state::ReviewStatus::Reviewed);
}

#[test_case::test_case(RepoType::Git; "git")]
#[test_case::test_case(RepoType::Jj; "jj")]
fn a_hunk_that_rewrites_approved_lines_stays_open(kind: RepoType) {
    let fixture = Fixture::new(kind);
    fixture.files.write("mixed.rs", &numbered(&[]));
    fixture.files.new_change("review");
    fixture
        .files
        .write("mixed.rs", &numbered(&[(2, "two"), (15, "fifteen")]));
    fixture.accept_hunk_at(1);
    // The approved "two" becomes a base-like line with a note.
    fixture
        .files
        .write("mixed.rs", &numbered(&[(2, "2 // note"), (15, "fifteen")]));
    let mut review = fixture.prepare();
    review.classify_line("mixed.rs", 2);

    let summary = review
        .apply(&fixture.repository, &fixture.tracker, &fixture.store)
        .unwrap();

    assert_eq!(summary.hunks, 0);
    assert_eq!(
        fixture.status(),
        review_state::ReviewStatus::PartiallyReviewed
    );
}
