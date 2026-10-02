use review_hunks::{Attribution, FileHunks, HunkMark, HunkSpan, LineSelection, Reviewed};
use review_repository::repository::{RepoType, Repository, Snapshot};
use review_state::{MarkResult, ReviewDiff, ReviewStatus, ReviewTracker};
use review_store::{LoadResult, PartialReview, ReviewStore};
use review_test_support::{
    ReviewRepositoryFixture, complete_repository_snapshot, repository_fixture,
};
use review_types::MarkAuthor;
use test_case::test_case;

/// Twenty lines; the change under review rewrites lines 2, 10 and 18, far
/// enough apart to make three hunks.
fn file(edits: &[(usize, &str)]) -> Vec<u8> {
    let mut lines = (1..=20)
        .map(|line| format!("line {line}"))
        .collect::<Vec<_>>();
    for (line, text) in edits {
        (*text).clone_into(&mut lines[*line]);
    }
    lines
        .join("\n")
        .into_bytes()
        .into_iter()
        .chain([b'\n'])
        .collect()
}

fn current() -> Vec<u8> {
    file(&[(1, "two"), (9, "ten"), (17, "eighteen")])
}

struct Review {
    files: Box<dyn ReviewRepositoryFixture>,
    repository: Repository,
    store: ReviewStore,
    tracker: ReviewTracker,
    _state: tempfile::TempDir,
}

impl Review {
    fn new(repository_type: RepoType) -> Self {
        let files = repository_fixture(repository_type);
        files.write("file.txt", &file(&[]));
        files.new_change("review");
        files.write("file.txt", &current());
        let state = tempfile::tempdir().unwrap();
        let repository = Repository::discover(files.root())
            .unwrap()
            .with_state_root(state.path());
        let tracker = ReviewTracker::new(
            repository.clone(),
            ReviewStore::open(state.path(), files.root()).unwrap(),
        );
        Self {
            store: ReviewStore::open(state.path(), files.root()).unwrap(),
            files,
            repository,
            tracker,
            _state: state,
        }
    }

    fn snapshot(&self) -> Snapshot {
        complete_repository_snapshot(&self.repository)
    }

    fn diff(&self) -> ReviewDiff {
        let snapshot = self.snapshot();
        self.tracker.diff(&snapshot, &snapshot.files[0]).unwrap()
    }

    fn hunks(&self) -> FileHunks {
        self.diff().hunks
    }

    fn status(&self) -> ReviewStatus {
        let snapshot = self.snapshot();
        let single = self.tracker.status(&snapshot, &snapshot.files[0]).unwrap();
        let grouped = self.tracker.statuses(&snapshot).unwrap()[0];
        assert_eq!(single, grouped, "single and grouped states agree");
        single.status
    }

    fn mark(&self, mark: &HunkMark) {
        let snapshot = self.snapshot();
        assert_eq!(
            self.tracker
                .mark_hunk(&snapshot, &snapshot.files[0], mark, &MarkAuthor::Reviewer)
                .unwrap(),
            MarkResult::Marked
        );
    }

    fn accept_first_open_hunk(&self) {
        self.mark(&HunkMark::Review(self.hunks().open[0].span.clone()));
    }

    fn record(&self) -> LoadResult {
        let snapshot = self.snapshot();
        self.store
            .load(snapshot.identity.review_unit(), b"file.txt")
            .unwrap()
    }
}

fn spans<'a>(spans: impl IntoIterator<Item = &'a HunkSpan>) -> Vec<(u32, u32)> {
    spans
        .into_iter()
        .map(|span| (span.new.start, span.new.end))
        .collect()
}

#[test_case(RepoType::Git; "git")]
#[test_case(RepoType::Jj; "jj")]
fn an_accepted_hunk_leaves_the_open_diff(repository_type: RepoType) {
    let review = Review::new(repository_type);

    review.accept_first_open_hunk();

    assert_eq!(review.status(), ReviewStatus::PartiallyReviewed);
    let hunks = review.hunks();
    assert_eq!(
        spans(hunks.open.iter().map(|hunk| &hunk.span)),
        [(9, 10), (17, 18)]
    );
    assert_eq!(
        spans(hunks.reviewed.iter().map(|hunk| &hunk.span)),
        [(1, 2)]
    );
    assert!(hunks.open.iter().all(|hunk| !hunk.since_review));
    let diff = String::from_utf8(review.diff().unified).unwrap();
    assert!(!diff.contains("+two"), "{diff}");
    assert!(diff.contains("+ten"), "{diff}");
}

#[test_case(RepoType::Git; "git")]
#[test_case(RepoType::Jj; "jj")]
fn accepting_every_hunk_reviews_the_whole_file(repository_type: RepoType) {
    let review = Review::new(repository_type);

    for _ in 0..3 {
        review.accept_first_open_hunk();
    }

    assert_eq!(review.status(), ReviewStatus::Reviewed);
    let LoadResult::Reviewed(record) = review.record() else {
        panic!("the file was not marked");
    };
    assert_eq!(record.partial, None);
}

#[test_case(RepoType::Git; "git")]
#[test_case(RepoType::Jj; "jj")]
fn reopening_the_only_reviewed_hunk_unreviews_the_file(repository_type: RepoType) {
    let review = Review::new(repository_type);
    review.accept_first_open_hunk();

    review.mark(&HunkMark::Unreview(review.hunks().reviewed[0].span.clone()));

    assert_eq!(review.status(), ReviewStatus::Unreviewed);
    assert_eq!(review.record(), LoadResult::Unreviewed);
}

#[test_case(RepoType::Git; "git")]
#[test_case(RepoType::Jj; "jj")]
fn a_reviewed_hunk_edited_later_shows_only_the_change_since_review(repository_type: RepoType) {
    let review = Review::new(repository_type);
    review.accept_first_open_hunk();
    review.accept_first_open_hunk();

    review.files.write(
        "file.txt",
        &file(&[(1, "two!"), (9, "ten"), (17, "eighteen")]),
    );

    let hunks = review.hunks();
    assert_eq!(
        hunks
            .open
            .iter()
            .map(|hunk| (hunk.span.new.start, hunk.since_review))
            .collect::<Vec<_>>(),
        [(1, true), (17, false)]
    );
    assert_eq!(
        spans(hunks.reviewed.iter().map(|hunk| &hunk.span)),
        [(9, 10)]
    );
    let diff = String::from_utf8(review.diff().unified).unwrap();
    assert!(diff.contains("-two\n+two!\n"), "{diff}");
    assert!(!diff.contains("-line 2\n"), "{diff}");
}

#[test_case(RepoType::Git; "git")]
#[test_case(RepoType::Jj; "jj")]
fn a_whole_file_mark_splits_into_hunks_after_an_edit(repository_type: RepoType) {
    let review = Review::new(repository_type);
    let snapshot = review.snapshot();
    review
        .tracker
        .mark(&snapshot, &snapshot.files[0], &MarkAuthor::Reviewer)
        .unwrap();
    review.files.write(
        "file.txt",
        &file(&[(1, "two"), (9, "TEN"), (17, "eighteen")]),
    );
    assert_eq!(review.status(), ReviewStatus::ChangedSinceReview);
    let hunks = review.hunks();
    assert_eq!(
        hunks
            .open
            .iter()
            .map(|hunk| (hunk.span.new.start, hunk.since_review))
            .collect::<Vec<_>>(),
        [(9, true)]
    );
    assert_eq!(
        spans(hunks.reviewed.iter().map(|hunk| &hunk.span)),
        [(1, 2), (17, 18)]
    );

    review.mark(&HunkMark::Unreview(hunks.reviewed[1].span.clone()));

    assert_eq!(review.status(), ReviewStatus::PartiallyReviewed);
    let hunks = review.hunks();
    assert_eq!(
        hunks
            .open
            .iter()
            .map(|hunk| (hunk.span.new.start, hunk.since_review))
            .collect::<Vec<_>>(),
        [(9, true), (17, false)]
    );
    assert_eq!(
        spans(hunks.reviewed.iter().map(|hunk| &hunk.span)),
        [(1, 2)]
    );
}

#[test_case(RepoType::Git; "git")]
#[test_case(RepoType::Jj; "jj")]
fn a_hunk_that_is_not_listed_cannot_be_marked(repository_type: RepoType) {
    let review = Review::new(repository_type);
    let snapshot = review.snapshot();

    let result = review.tracker.mark_hunk(
        &snapshot,
        &snapshot.files[0],
        &HunkMark::Unreview(HunkSpan {
            old: 1..2,
            new: 1..2,
        }),
        &MarkAuthor::Reviewer,
    );

    assert!(result.is_err());
    assert_eq!(review.record(), LoadResult::Unreviewed);
}

#[test]
fn reviewed_hunks_stay_reviewed_when_the_base_changes_elsewhere() {
    let review = Review::new(RepoType::Jj);
    review.accept_first_open_hunk();
    let reviewed_change = review.files.revision_id();

    review.files.edit("@-");
    review
        .files
        .write("file.txt", &file(&[(5, "six upstream")]));
    review.files.edit(&reviewed_change);

    assert_eq!(review.status(), ReviewStatus::PartiallyReviewed);
    let hunks = review.hunks();
    assert_eq!(
        spans(hunks.reviewed.iter().map(|hunk| &hunk.span)),
        [(1, 2)]
    );
    assert_eq!(
        spans(hunks.open.iter().map(|hunk| &hunk.span)),
        [(9, 10), (17, 18)]
    );
    assert!(hunks.open.iter().all(|hunk| !hunk.since_review));
}

#[test_case(RepoType::Git; "git")]
#[test_case(RepoType::Jj; "jj")]
fn a_reviewed_change_that_meets_a_new_base_change_reopens(repository_type: RepoType) {
    let review = Review::new(repository_type);
    let snapshot = review.snapshot();
    // The mark was made on a base whose line 2 has changed since; line 10
    // was reviewed too, on lines the base change did not touch.
    review
        .store
        .mark_partial(
            snapshot.identity.review_unit(),
            b"file.txt",
            snapshot.identity.snapshot_id(),
            PartialReview {
                base: file(&[(1, "old two")]),
                reviewed: Reviewed {
                    text: file(&[(1, "two"), (9, "ten")]),
                    attribution: Attribution::default(),
                },
            },
        )
        .unwrap();

    assert_eq!(review.status(), ReviewStatus::PartiallyReviewed);
    let hunks = review.hunks();
    assert_eq!(
        spans(hunks.reviewed.iter().map(|hunk| &hunk.span)),
        [(9, 10)]
    );
    assert_eq!(
        spans(hunks.open.iter().map(|hunk| &hunk.span)),
        [(1, 2), (17, 18)]
    );
}

#[test_case(RepoType::Git; "git")]
#[test_case(RepoType::Jj; "jj")]
fn a_file_whose_reviewed_changes_all_meet_base_changes_is_unreviewed(repository_type: RepoType) {
    let review = Review::new(repository_type);
    let snapshot = review.snapshot();
    review
        .store
        .mark_partial(
            snapshot.identity.review_unit(),
            b"file.txt",
            snapshot.identity.snapshot_id(),
            PartialReview {
                base: file(&[(1, "old two")]),
                reviewed: Reviewed {
                    text: file(&[(1, "two")]),
                    attribution: Attribution::default(),
                },
            },
        )
        .unwrap();

    assert_eq!(review.status(), ReviewStatus::Unreviewed);
    assert!(review.hunks().reviewed.is_empty());
}

#[test_case(RepoType::Git; "git")]
#[test_case(RepoType::Jj; "jj")]
fn hunks_whose_changed_lines_pass_a_check_are_accepted_together(repository_type: RepoType) {
    let review = Review::new(repository_type);
    let snapshot = review.snapshot();

    let accepted = review
        .tracker
        .review_hunks_where(
            &snapshot,
            &snapshot.files[0],
            false,
            &MarkAuthor::Jev,
            |lines| lines.added.iter().all(|line| *line != 9),
        )
        .unwrap();

    assert_eq!(accepted, 2);
    assert_eq!(review.status(), ReviewStatus::PartiallyReviewed);
    assert_eq!(
        spans(review.hunks().open.iter().map(|hunk| &hunk.span)),
        [(9, 10)]
    );
}

#[test_case(RepoType::Git; "git")]
#[test_case(RepoType::Jj; "jj")]
fn a_check_every_hunk_passes_leaves_a_file_with_more_changes_unreviewed(repository_type: RepoType) {
    let review = Review::new(repository_type);
    let snapshot = review.snapshot();

    let accepted = review
        .tracker
        .review_hunks_where(
            &snapshot,
            &snapshot.files[0],
            false,
            &MarkAuthor::Jev,
            |_| true,
        )
        .unwrap();

    assert_eq!(accepted, 0);
    assert_eq!(review.record(), LoadResult::Unreviewed);
}

#[test_case(RepoType::Git; "git")]
#[test_case(RepoType::Jj; "jj")]
fn a_check_every_hunk_passes_may_review_a_file_that_changes_only_lines(repository_type: RepoType) {
    let review = Review::new(repository_type);
    review.accept_first_open_hunk();
    let snapshot = review.snapshot();

    let accepted = review
        .tracker
        .review_hunks_where(
            &snapshot,
            &snapshot.files[0],
            true,
            &MarkAuthor::Jev,
            |_| true,
        )
        .unwrap();

    assert_eq!(accepted, 2);
    assert_eq!(review.status(), ReviewStatus::Reviewed);
}

fn selection(removed: &[u32], added: &[u32]) -> LineSelection {
    LineSelection {
        removed: removed.iter().copied().collect(),
        added: added.iter().copied().collect(),
    }
}

impl Review {
    fn accept_lines(&self, selection: &LineSelection, author: &MarkAuthor) -> MarkResult {
        let snapshot = self.snapshot();
        self.tracker
            .accept_lines(&snapshot, &snapshot.files[0], selection, author)
            .unwrap()
    }
}

#[test_case(RepoType::Git; "git")]
#[test_case(RepoType::Jj; "jj")]
fn accepting_one_side_of_a_rewritten_line_splits_its_hunk(repository_type: RepoType) {
    let review = Review::new(repository_type);

    // Accept "two" but not the removal of "line 2" it replaces.
    assert_eq!(
        review.accept_lines(&selection(&[], &[1]), &MarkAuthor::Jev),
        MarkResult::Marked
    );

    assert_eq!(review.status(), ReviewStatus::PartiallyReviewed);
    let hunks = review.hunks();
    assert_eq!(
        spans(hunks.open.iter().map(|hunk| &hunk.span)),
        [(1, 1), (9, 10), (17, 18)]
    );
    assert_eq!(
        hunks.count().map(|count| (count.reviewed, count.total)),
        Some((1, 6))
    );
    let LoadResult::Reviewed(record) = review.record() else {
        panic!("the lines were marked");
    };
    let partial = record.partial.expect("some lines stay open");
    assert_eq!(
        partial.reviewed.attribution.uniform_author(),
        Some(&MarkAuthor::Jev)
    );
}

#[test_case(RepoType::Git; "git")]
#[test_case(RepoType::Jj; "jj")]
fn a_file_reviewed_by_several_authors_keeps_who_reviewed_which_line(repository_type: RepoType) {
    let review = Review::new(repository_type);
    review.accept_first_open_hunk();

    review.accept_lines(&selection(&[9, 17], &[9, 17]), &MarkAuthor::Jev);

    assert_eq!(review.status(), ReviewStatus::Reviewed);
    let LoadResult::Reviewed(record) = review.record() else {
        panic!("the file was marked");
    };
    let partial = record
        .partial
        .expect("a partial mark keeps the authors of a whole review");
    let attribution = &partial.reviewed.attribution;
    assert_eq!(attribution.added_by(1), &MarkAuthor::Reviewer);
    assert_eq!(attribution.added_by(9), &MarkAuthor::Jev);
    assert_eq!(attribution.removed_by(17), &MarkAuthor::Jev);

    // Reopening Jev's lines leaves the reviewer's.
    let snapshot = review.snapshot();
    assert_eq!(
        review
            .tracker
            .reopen_lines(
                &snapshot,
                &snapshot.files[0],
                &selection(&[9, 17], &[9, 17])
            )
            .unwrap(),
        MarkResult::Marked
    );
    assert_eq!(
        spans(review.hunks().reviewed.iter().map(|hunk| &hunk.span)),
        [(1, 2)]
    );
}

#[test_case(RepoType::Git; "git")]
#[test_case(RepoType::Jj; "jj")]
fn one_author_reviewing_every_line_marks_the_whole_file(repository_type: RepoType) {
    let review = Review::new(repository_type);

    review.accept_lines(&selection(&[1, 9, 17], &[1, 9, 17]), &MarkAuthor::Jev);

    let LoadResult::Reviewed(record) = review.record() else {
        panic!("the file was marked");
    };
    assert_eq!(record.partial, None);
    assert_eq!(record.author, MarkAuthor::Jev);
}

#[test_case(RepoType::Git; "git")]
#[test_case(RepoType::Jj; "jj")]
fn a_selection_of_unchanged_lines_marks_nothing(repository_type: RepoType) {
    let review = Review::new(repository_type);

    assert_eq!(
        review.accept_lines(&selection(&[3], &[3]), &MarkAuthor::Reviewer),
        MarkResult::NothingToMark
    );
    assert_eq!(review.record(), LoadResult::Unreviewed);
}

#[test_case(RepoType::Git; "git")]
#[test_case(RepoType::Jj; "jj")]
fn a_files_lines_name_what_is_open_and_who_reviewed_the_rest(repository_type: RepoType) {
    let review = Review::new(repository_type);
    review.accept_lines(&selection(&[1], &[1]), &MarkAuthor::Jev);

    let snapshot = review.snapshot();
    let lines = review.tracker.lines(&snapshot, &snapshot.files[0]).unwrap();

    assert_eq!(
        lines
            .open
            .iter()
            .map(|open| (open.lines.removed.clone(), open.lines.added.clone()))
            .collect::<Vec<_>>(),
        [(vec![9], vec![9]), (vec![17], vec![17])]
    );
    assert_eq!(lines.reviewed.removed, [(1, MarkAuthor::Jev)].into());
    assert_eq!(lines.reviewed.added, [(1, MarkAuthor::Jev)].into());
}

#[test_case(RepoType::Git; "git")]
#[test_case(RepoType::Jj; "jj")]
fn lines_a_partial_mark_left_open_change_since_review_only_when_the_file_does(
    repository_type: RepoType,
) {
    let review = Review::new(repository_type);
    // Accept the removal of "line 2" but not the "two" replacing it.
    review.accept_lines(&selection(&[1], &[]), &MarkAuthor::Reviewer);
    let since_review = |review: &Review| {
        review
            .hunks()
            .open
            .iter()
            .map(|hunk| (hunk.span.new.start, hunk.since_review))
            .collect::<Vec<_>>()
    };
    assert_eq!(since_review(&review), [(1, false), (9, false), (17, false)]);

    review.files.write("other.txt", b"another file\n");
    assert_eq!(since_review(&review), [(1, false), (9, false), (17, false)]);

    review.files.write(
        "file.txt",
        &file(&[(1, "TWO"), (9, "ten"), (17, "eighteen")]),
    );
    assert_eq!(since_review(&review), [(1, true), (9, false), (17, false)]);
}
