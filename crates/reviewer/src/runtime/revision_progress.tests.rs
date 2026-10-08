use review_repository::repository::{
    ChangeId, RepoType, Repository, RevisionHistoryLine, SnapshotId,
};
use review_state::ReviewTracker;
use review_store::ReviewStore;
use review_test_support::{complete_repository_snapshot, repository_fixture};
use review_types::MarkAuthor;
use tempfile::TempDir;
use ui_events::RevisionHistoryLoadId;

use super::HistoryProgress;

fn line(id: &str, current: bool, immutable: bool) -> RevisionHistoryLine {
    RevisionHistoryLine {
        text: id.to_owned(),
        plain_text: id.to_owned(),
        graph_end: Some(0),
        short_change_id: Some(id.to_owned()),
        change_id: Some(ChangeId::from(id.to_owned())),
        commit_id: Some(SnapshotId::from(format!("{id}-commit"))),
        is_current: current,
        is_immutable: immutable,
    }
}

fn graph_only() -> RevisionHistoryLine {
    RevisionHistoryLine {
        text: "│".to_owned(),
        plain_text: "│".to_owned(),
        graph_end: None,
        short_change_id: None,
        change_id: None,
        commit_id: None,
        is_current: false,
        is_immutable: false,
    }
}

fn drain(progress: &mut HistoryProgress) -> Vec<String> {
    std::iter::from_fn(|| progress.next())
        .map(|(_, change_id)| change_id.as_str().to_owned())
        .collect()
}

/// A review store of its own, for histories whose revisions are never computed.
fn store() -> (TempDir, ReviewStore) {
    let state = tempfile::tempdir().unwrap();
    let store = ReviewStore::open(state.path(), state.path()).unwrap();
    (state, store)
}

/// A jj repository whose change `review` holds two one-line files, `first.txt` reviewed, and
/// whose working copy is a change above it.
struct Reviewed {
    _files: Box<dyn review_test_support::ReviewRepositoryFixture>,
    _state: TempDir,
    repository: Repository,
    store: ReviewStore,
    tracker: ReviewTracker,
    review: ChangeId,
    commit: String,
}

impl Reviewed {
    fn new() -> Self {
        let files = repository_fixture(RepoType::Jj);
        files.new_change("review");
        files.write("first.txt", b"one\n");
        files.write("second.txt", b"two\n");
        let state = tempfile::tempdir().unwrap();
        let repository = Repository::discover(files.root())
            .unwrap()
            .with_state_root(state.path());
        let store = ReviewStore::open(state.path(), files.root()).unwrap();
        let tracker = ReviewTracker::new(repository.clone(), store.clone());
        let reviewed = complete_repository_snapshot(&repository);
        let first = reviewed
            .files
            .iter()
            .find(|file| file.display_path == "first.txt")
            .unwrap();
        tracker
            .mark(&reviewed, first, &MarkAuthor::Reviewer)
            .unwrap();
        // The working copy moves on: the revision reviewed is now one below it.
        files.new_change("next");
        files.write("third.txt", b"three\n");
        Self {
            _files: files,
            _state: state,
            repository,
            store,
            tracker,
            review: ChangeId::from(reviewed.identity.review_unit().as_str().to_owned()),
            commit: reviewed.identity.snapshot_id().to_owned(),
        }
    }

    /// The history row of the reviewed change, at `commit`.
    fn line(&self, commit: &str) -> RevisionHistoryLine {
        RevisionHistoryLine {
            commit_id: Some(SnapshotId::from(commit.to_owned())),
            ..line(self.review.as_str(), true, false)
        }
    }

    fn compute(&self, progress: &mut HistoryProgress) -> Option<u64> {
        progress
            .compute(&self.repository, &self.tracker, &self.store, &self.review)
            .map(review_state::ReviewProgress::percent)
    }
}

#[test]
fn revisions_come_nearest_to_the_current_one_first_without_immutable_or_graph_rows() {
    let (_state, store) = store();
    let lines = [
        line("top", false, false),
        line("above", false, false),
        graph_only(),
        line("current", true, false),
        line("below", false, false),
        line("trunk", false, true),
    ];
    let mut progress = HistoryProgress::default();
    assert!(
        progress
            .start(RevisionHistoryLoadId::new(1), &lines, &store)
            .queue
    );
    assert_eq!(drain(&mut progress), ["current", "below", "above", "top"]);
}

#[test]
fn one_command_is_queued_at_a_time_until_none_is_left() {
    let (_state, store) = store();
    let lines = [line("a", true, false), line("b", false, false)];
    let mut progress = HistoryProgress::default();
    assert!(
        progress
            .start(RevisionHistoryLoadId::new(1), &lines, &store)
            .queue
    );
    // A second history while the command is queued queues no other.
    let second = progress.start(RevisionHistoryLoadId::new(2), &lines, &store);
    assert!(!second.queue);
    let (load_id, first) = progress.next().unwrap();
    assert_eq!(
        (load_id, first.as_str()),
        (RevisionHistoryLoadId::new(2), "a")
    );
    assert!(progress.queue());
    assert!(!progress.queue());
    assert_eq!(progress.next().unwrap().1.as_str(), "b");
    assert!(!progress.queue());
    assert!(progress.next().is_none());
}

#[test]
fn a_history_without_revisions_to_compute_queues_nothing() {
    let (_state, store) = store();
    let mut progress = HistoryProgress::default();
    let started = progress.start(
        RevisionHistoryLoadId::new(1),
        &[line("trunk", true, true)],
        &store,
    );
    assert!(!started.queue);
    assert!(progress.next().is_none());
}

#[test]
fn every_revision_of_a_long_history_gets_its_share() {
    let (_state, store) = store();
    let mut lines = vec![line("current", true, false)];
    lines.extend((0..60).map(|row| line(&format!("below-{row}"), false, false)));
    let mut progress = HistoryProgress::default();
    progress.start(RevisionHistoryLoadId::new(1), &lines, &store);

    let revisions = drain(&mut progress);

    assert_eq!(revisions.len(), 61);
    assert_eq!(revisions.last().map(String::as_str), Some("below-59"));
}

#[test]
fn a_revision_below_the_working_copy_counts_its_own_review_marks() {
    let reviewed = Reviewed::new();
    let mut progress = HistoryProgress::default();

    assert_eq!(reviewed.compute(&mut progress), Some(50));
}

#[test]
fn a_share_stays_known_while_its_commit_and_marks_are_the_same() {
    let reviewed = Reviewed::new();
    let mut progress = HistoryProgress::default();
    reviewed.compute(&mut progress);

    let again = progress.start(
        RevisionHistoryLoadId::new(1),
        &[reviewed.line(&reviewed.commit)],
        &reviewed.store,
    );

    let known = again
        .known
        .iter()
        .map(|(change_id, share)| (change_id.clone(), share.percent()))
        .collect::<Vec<_>>();
    assert_eq!(known, [(reviewed.review.clone(), 50)]);
    assert!(!again.queue);
}

#[test]
fn a_share_stays_known_for_the_row_jj_log_gives_its_revision() {
    let reviewed = Reviewed::new();
    let mut progress = HistoryProgress::default();
    reviewed.compute(&mut progress);
    let history = reviewed.repository.revision_history().unwrap();

    let again = progress.start(RevisionHistoryLoadId::new(1), &history, &reviewed.store);

    assert!(
        again
            .known
            .iter()
            .any(|(change_id, _)| change_id == &reviewed.review),
        "{history:?}"
    );
}

#[test]
fn a_share_is_computed_again_once_the_commit_or_the_marks_change() {
    let reviewed = Reviewed::new();
    let mut progress = HistoryProgress::default();
    reviewed.compute(&mut progress);

    let rewritten = progress.start(
        RevisionHistoryLoadId::new(1),
        &[reviewed.line("another-commit")],
        &reviewed.store,
    );
    assert!(rewritten.known.is_empty() && rewritten.queue);

    progress.next();
    reviewed.compute(&mut progress);
    // Any mark of the change, from any process, changes the stamp the share was kept with.
    reviewed
        .store
        .unreview(reviewed.review.review_unit(), b"first.txt")
        .unwrap();
    let unmarked = progress.start(
        RevisionHistoryLoadId::new(2),
        &[reviewed.line(&reviewed.commit)],
        &reviewed.store,
    );
    assert!(unmarked.known.is_empty() && unmarked.queue);
    progress.next();
    assert_eq!(reviewed.compute(&mut progress), Some(0));
}

#[test]
fn a_revision_that_cannot_be_read_has_no_share() {
    let reviewed = Reviewed::new();
    let mut progress = HistoryProgress::default();
    let missing = ChangeId::from("kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk".to_owned());

    let share = progress.compute(
        &reviewed.repository,
        &reviewed.tracker,
        &reviewed.store,
        &missing,
    );

    assert_eq!(share, None);
}
