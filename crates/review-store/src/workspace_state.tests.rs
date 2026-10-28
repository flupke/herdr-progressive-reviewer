use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use review_explore::{Comparison, Exploration, ExploreHistory, ExploreRound};
use review_threads::{Post, ReviewThreads};
use review_types::{MarkAuthor, ReviewUnit};
use tempfile::TempDir;

use super::WorkspaceState;
use crate::stored_fixtures::{DRAFTS_V1, THREADS_V4};
use crate::{LoadResult, ReviewStore};

const MAIN: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const TASK: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

struct Fixture {
    _temporary: TempDir,
    state: PathBuf,
    shared: ReviewStore,
}

impl Fixture {
    fn new() -> Self {
        let temporary = TempDir::new().unwrap();
        let state = temporary.path().join("state");
        let repository = temporary.path().join("repository/.jj/repo");
        fs::create_dir_all(&repository).unwrap();
        let shared = ReviewStore::open(&state, &repository).unwrap();
        Self {
            _temporary: temporary,
            state,
            shared,
        }
    }

    /// The store of a per-workspace state with `key`, as builds before ADR 0013 kept it.
    fn old(&self, key: &str) -> ReviewStore {
        let store = ReviewStore {
            state_root: self.state.clone(),
            repository_dir: self.state.join(key),
            thread_sources: Arc::default(),
        };
        store.create_dir(&store.repository_dir).unwrap();
        store
    }

    fn state(&self, key: &str) -> WorkspaceState {
        WorkspaceState::list(&self.state)
            .unwrap()
            .into_iter()
            .find(|state| state.key() == key)
            .unwrap()
    }
}

fn round(unit: &str) -> ExploreRound {
    ExploreRound::new(Exploration::new(Arc::new(Comparison {
        repository_root: "/workspace".into(),
        checkpoint: review_source::ReviewCheckpoint::new(unit, "checkpoint"),
        files: vec![],
        context: vec![],
        diffs: vec![],
        manifest: vec![],
        sources: vec![],
        base: None,
    })))
}

/// Save `round` in `store` with the history `rounds`, its file dated `saved`.
fn save_round(store: &ReviewStore, round: &ExploreRound, rounds: Vec<String>, saved: SystemTime) {
    let unit = round.exploration.comparison.checkpoint.review_unit.clone();
    let records = store.lock_explore(&unit).unwrap();
    records.create_round(round).unwrap();
    records
        .save_history(&ExploreHistory {
            rounds,
            ..ExploreHistory::default()
        })
        .unwrap();
    let path = store
        .explore_path(&unit, &round.exploration.instance)
        .unwrap();
    fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(saved)
        .unwrap();
}

/// Every file under `directory` with its bytes, but the locks.
fn files(directory: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut found = BTreeMap::new();
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            found.extend(files(&path));
        } else if path.extension().is_none_or(|extension| extension != "lock")
            && path.file_name().is_some_and(|name| name != "lock")
        {
            found.insert(path.clone(), fs::read(&path).unwrap());
        }
    }
    found
}

fn reviewed_at(store: &ReviewStore, unit: &ReviewUnit, path: &[u8]) -> Option<String> {
    match store.load(unit, path).unwrap() {
        LoadResult::Reviewed(record) => Some(record.reviewed_at),
        _ => None,
    }
}

#[test]
fn only_folders_named_by_a_workspace_key_are_per_workspace_states() {
    let fixture = Fixture::new();
    fixture.old(MAIN);
    for name in ["git", "explore-page", &"A".repeat(64), &"a".repeat(63)] {
        fs::create_dir_all(fixture.state.join(name)).unwrap();
    }
    fs::write(fixture.state.join("b".repeat(64)), "a file").unwrap();

    let keys = WorkspaceState::list(&fixture.state)
        .unwrap()
        .into_iter()
        .map(|state| state.key().to_owned())
        .collect::<Vec<_>>();

    assert_eq!(keys, [MAIN]);
    assert_eq!(
        WorkspaceState::key_of(Path::new("/home/user/project")).unwrap(),
        "9dad1e4e08b0b11cbcd860257e8bdfa6b8e5f01790e10a6a0b1f4870c13e686b"
    );
}

#[test]
fn a_state_names_the_reviews_of_its_marks_threads_drafts_and_rounds() {
    let fixture = Fixture::new();
    let old = fixture.old(MAIN);
    old.mark(&"marked".into(), b"a.rs", COMMIT, &MarkAuthor::Reviewer)
        .unwrap();
    old.write_context_fixture();
    old.write_json_fixture(&old.threads_fixture_path(), THREADS_V4);
    old.write_json_fixture(
        &old.review_record_path("drafts", &"drafted".into()).unwrap(),
        &DRAFTS_V1.replace(r#""review_unit":"change""#, r#""review_unit":"drafted""#),
    );
    save_round(&old, &round("explored"), Vec::new(), SystemTime::now());

    let units = fixture.state(MAIN).review_units().unwrap();

    let units = units.iter().map(ReviewUnit::as_str).collect::<Vec<_>>();
    assert_eq!(units, ["change", "drafted", "explored", "marked"]);
}

#[test]
fn the_later_mark_of_each_file_wins_and_marks_of_one_side_are_kept() {
    let fixture = Fixture::new();
    let unit: ReviewUnit = "change".into();
    let old = fixture.old(MAIN);
    old.mark(&unit, b"old-only.rs", COMMIT, &MarkAuthor::Reviewer)
        .unwrap();
    old.mark(&unit, b"older-in-task.rs", COMMIT, &MarkAuthor::Reviewer)
        .unwrap();
    let shared = &fixture.shared;
    shared
        .mark(&unit, b"shared-only.rs", COMMIT, &MarkAuthor::Reviewer)
        .unwrap();
    shared
        .mark(&unit, b"older-in-task.rs", COMMIT, &MarkAuthor::Jev)
        .unwrap();
    old.mark(&unit, b"newer-in-task.rs", COMMIT, &MarkAuthor::Reviewer)
        .unwrap();
    shared
        .mark(&unit, b"newer-in-task.rs", COMMIT, &MarkAuthor::Jev)
        .unwrap();
    // Marks written within one second share their time: date the shared ones apart.
    let shared_record = shared.record_path(&unit, b"newer-in-task.rs");
    let text = fs::read_to_string(&shared_record).unwrap();
    let written = reviewed_at(shared, &unit, b"newer-in-task.rs").unwrap();
    fs::write(
        &shared_record,
        text.replace(&written, "2000-01-01T00:00:00Z"),
    )
    .unwrap();
    let old_record = old.record_path(&unit, b"older-in-task.rs");
    let text = fs::read_to_string(&old_record).unwrap();
    let written = reviewed_at(&old, &unit, b"older-in-task.rs").unwrap();
    fs::write(&old_record, text.replace(&written, "2000-01-01T00:00:00Z")).unwrap();
    let stamp = shared.marks_stamp(&unit).unwrap();

    let unreadable = shared.merge_workspace_state(&fixture.state(MAIN)).unwrap();

    assert!(unreadable.is_empty());
    let author = |path: &[u8]| match shared.load(&unit, path).unwrap() {
        LoadResult::Reviewed(record) => Some(record.author),
        _ => None,
    };
    assert_eq!(author(b"old-only.rs"), Some(MarkAuthor::Reviewer));
    assert_eq!(author(b"shared-only.rs"), Some(MarkAuthor::Reviewer));
    assert_eq!(author(b"older-in-task.rs"), Some(MarkAuthor::Jev));
    assert_eq!(author(b"newer-in-task.rs"), Some(MarkAuthor::Reviewer));
    assert_ne!(shared.marks_stamp(&unit).unwrap(), stamp);
}

#[test]
fn threads_and_drafts_come_over_with_their_contexts_and_stay_pending() {
    let fixture = Fixture::new();
    let unit: ReviewUnit = "change".into();
    let old = fixture.old(MAIN);
    old.write_context_fixture();
    old.write_json_fixture(&old.threads_fixture_path(), THREADS_V4);
    old.write_json_fixture(&old.review_record_path("drafts", &unit).unwrap(), DRAFTS_V1);
    let mut shared_threads = ReviewThreads::new(unit.clone());
    shared_threads
        .post(Post::start(
            crate::stored_fixtures::code_source(&old.load_threads(&unit).unwrap().threads()[0])
                .anchor
                .clone(),
            "+fn main() {}".into(),
            "Asked in the shared state".into(),
        ))
        .unwrap();
    fixture
        .shared
        .update_threads(&unit, |threads| {
            *threads = shared_threads;
            Ok(())
        })
        .unwrap();

    fixture
        .shared
        .merge_workspace_state(&fixture.state(MAIN))
        .unwrap();
    fs::remove_dir_all(fixture.state.join(MAIN)).unwrap();

    let reopened = ReviewStore::open(
        &fixture.state,
        fixture.state.parent().unwrap().join("repository/.jj/repo"),
    )
    .unwrap();
    let threads = reopened.load_threads(&unit).unwrap();
    let texts = threads
        .threads()
        .iter()
        .map(|thread| thread.messages[0].text.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        texts,
        [
            "Asked in the shared state",
            "Why \"this\"?\nExplain.",
            "Pending"
        ]
    );
    assert_eq!(threads.new_messages().len(), 2);
    let drafts = reopened.load_drafts(&unit).unwrap();
    let drafts = drafts
        .drafts()
        .iter()
        .map(|draft| draft.text.as_str())
        .collect::<Vec<_>>();
    assert_eq!(drafts, ["Unposted \"reply\"\n", "New thread"]);
}

#[test]
fn a_history_of_one_side_is_kept_and_two_histories_become_read_only() {
    let fixture = Fixture::new();
    let old = fixture.old(MAIN);
    let now = SystemTime::now();
    let alone = round("alone");
    save_round(&old, &alone, vec![alone.exploration.instance.clone()], now);
    let first = round("both");
    let second = round("both");
    save_round(
        &fixture.shared,
        &second,
        vec![second.exploration.instance.clone()],
        now,
    );
    save_round(
        &old,
        &first,
        vec![first.exploration.instance.clone()],
        now - Duration::from_secs(60),
    );

    fixture
        .shared
        .merge_workspace_state(&fixture.state(MAIN))
        .unwrap();

    let shared = &fixture.shared;
    assert_eq!(
        shared.load_explore_history(&"alone".into()).unwrap(),
        ExploreHistory {
            rounds: vec![alone.exploration.instance.clone()],
            ..ExploreHistory::default()
        }
    );
    assert_eq!(
        shared.load_explore_history(&"both".into()).unwrap(),
        ExploreHistory {
            rounds: vec![
                first.exploration.instance.clone(),
                second.exploration.instance.clone()
            ],
            latest_editable: false,
            ..ExploreHistory::default()
        }
    );
    assert!(
        shared
            .load_explore(&"both".into(), &first.exploration.instance)
            .unwrap()
            .is_some()
    );
}

#[test]
fn merging_a_state_again_changes_nothing() {
    let fixture = Fixture::new();
    let unit: ReviewUnit = "change".into();
    let old = fixture.old(MAIN);
    old.mark(&unit, b"a.rs", COMMIT, &MarkAuthor::Reviewer)
        .unwrap();
    old.write_context_fixture();
    old.write_json_fixture(&old.threads_fixture_path(), THREADS_V4);
    old.write_json_fixture(&old.review_record_path("drafts", &unit).unwrap(), DRAFTS_V1);
    let explored = round("change");
    save_round(
        &old,
        &explored,
        vec![explored.exploration.instance.clone()],
        SystemTime::now(),
    );
    fixture
        .shared
        .merge_workspace_state(&fixture.state(MAIN))
        .unwrap();
    let mut merged = files(fixture.shared.directory());
    merged.retain(|path, _| !path.ends_with("marks-stamp"));

    fixture
        .shared
        .merge_workspace_state(&fixture.state(MAIN))
        .unwrap();

    let mut again = files(fixture.shared.directory());
    again.retain(|path, _| !path.ends_with("marks-stamp"));
    assert_eq!(again, merged);
}

#[test]
fn an_unreadable_record_is_reported_and_the_others_are_merged() {
    let fixture = Fixture::new();
    let old = fixture.old(MAIN);
    old.mark(&"change".into(), b"a.rs", COMMIT, &MarkAuthor::Reviewer)
        .unwrap();
    let broken = old.threads_fixture_path();
    fs::create_dir_all(broken.parent().unwrap()).unwrap();
    fs::write(&broken, "not compressed").unwrap();

    let unreadable = fixture
        .shared
        .merge_workspace_state(&fixture.state(MAIN))
        .unwrap();

    assert_eq!(unreadable, [broken]);
    assert!(reviewed_at(&fixture.shared, &"change".into(), b"a.rs").is_some());
}

#[test]
fn a_merged_state_moves_aside_and_names_the_folder_it_went_to() {
    let fixture = Fixture::new();
    fixture.old(MAIN);
    fixture.old(TASK);

    let moved = fixture
        .shared
        .retire_workspace_state(&fixture.state(MAIN))
        .unwrap();
    fixture.old(MAIN);
    let again = fixture
        .shared
        .retire_workspace_state(&fixture.state(MAIN))
        .unwrap();

    let merged = fixture.state.join("merged-workspaces");
    assert_eq!(moved, merged.join(MAIN));
    assert_eq!(again, merged.join(format!("{MAIN}-2")));
    let folder = fixture
        .shared
        .directory()
        .strip_prefix(&fixture.state)
        .unwrap()
        .to_owned();
    assert_eq!(
        fs::read_to_string(moved.join("merged-into")).unwrap(),
        format!("{}\n", folder.display())
    );
    let left = WorkspaceState::list(&fixture.state).unwrap();
    assert_eq!(
        left.iter().map(WorkspaceState::key).collect::<Vec<_>>(),
        [TASK]
    );
}
