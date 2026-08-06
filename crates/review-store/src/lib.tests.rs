use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;

use tempfile::TempDir;

use super::{LoadResult, OutputTarget, ReviewStore, StateKey};

struct Fixture {
    temporary: TempDir,
    state: std::path::PathBuf,
    repository: std::path::PathBuf,
    change: String,
}

impl Fixture {
    fn new() -> Self {
        let temporary = TempDir::new().unwrap();
        let state = temporary.path().join("state");
        let repository = temporary.path().join("repository");
        fs::create_dir(&repository).unwrap();
        Self {
            temporary,
            state,
            repository,
            change: "a".repeat(64),
        }
    }

    fn store(&self) -> ReviewStore {
        ReviewStore::open(&self.state, &self.repository).unwrap()
    }
}

#[test]
fn paths_round_trip_and_keys_are_stable() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let baseline = "b".repeat(64);

    for path in [b"src/lib.rs".to_vec(), b"invalid-\xff".to_vec()] {
        store.mark(&fixture.change, &path, &baseline).unwrap();
        let LoadResult::Reviewed(record) = store.load(&fixture.change, &path).unwrap() else {
            panic!("record was not loaded");
        };
        assert_eq!(record.path, path);
        assert_eq!(record.baseline_commit_id, baseline);
    }

    assert_eq!(
        StateKey::hash(b"src/lib.rs").0,
        StateKey::hash(b"src/lib.rs").0
    );
}

#[test]
fn roots_and_paths_have_separate_state() {
    let fixture = Fixture::new();
    let other_repository = fixture.temporary.path().join("other");
    fs::create_dir(&other_repository).unwrap();
    let first = fixture.store();
    let second = ReviewStore::open(&fixture.state, other_repository).unwrap();
    let baseline = "b".repeat(64);
    let left = b"left".to_vec();
    let right = b"right".to_vec();

    first.mark(&fixture.change, &left, &baseline).unwrap();
    first.mark(&fixture.change, &right, &baseline).unwrap();

    assert!(matches!(
        second.load(&fixture.change, &left).unwrap(),
        LoadResult::Unreviewed
    ));
    assert!(matches!(
        first.load(&fixture.change, &left).unwrap(),
        LoadResult::Reviewed(_)
    ));
    assert!(matches!(
        first.load(&fixture.change, &right).unwrap(),
        LoadResult::Reviewed(_)
    ));
}

#[test]
fn settings_are_shared_between_repositories() {
    let fixture = Fixture::new();
    let other_repository = fixture.temporary.path().join("other");
    fs::create_dir(&other_repository).unwrap();
    fixture.store().save_file_pane_width(42).unwrap();
    fixture
        .store()
        .save_output_target(OutputTarget::Clipboard)
        .unwrap();

    let settings = ReviewStore::open(&fixture.state, other_repository).unwrap();
    assert_eq!(settings.file_pane_width().unwrap(), Some(42));
    assert_eq!(settings.output_target().unwrap(), OutputTarget::Clipboard);
}

#[test]
fn concurrent_writers_leave_one_complete_record() {
    let fixture = Fixture::new();
    let path = b"shared".to_vec();
    fixture
        .store()
        .mark(&fixture.change, &path, &"d".repeat(64))
        .unwrap();
    let barrier = Arc::new(Barrier::new(3));
    let completed = Arc::new(AtomicUsize::new(0));
    let mut writers = Vec::new();
    for digit in ['b', 'c'] {
        let state = fixture.state.clone();
        let repository = fixture.repository.clone();
        let change = fixture.change.clone();
        let path = path.clone();
        let barrier = Arc::clone(&barrier);
        let completed = Arc::clone(&completed);
        writers.push(thread::spawn(move || {
            let store = ReviewStore::open(state, repository).unwrap();
            let baseline = digit.to_string().repeat(64);
            barrier.wait();
            for _ in 0..50 {
                store.mark(&change, &path, &baseline).unwrap();
            }
            completed.fetch_add(1, Ordering::Release);
        }));
    }
    barrier.wait();
    let reader = fixture.store();
    while completed.load(Ordering::Acquire) != 2 {
        assert!(matches!(
            reader.load(&fixture.change, &path).unwrap(),
            LoadResult::Reviewed(_)
        ));
    }
    for writer in writers {
        writer.join().unwrap();
    }

    let LoadResult::Reviewed(record) = fixture.store().load(&fixture.change, &path).unwrap() else {
        panic!("record was not loaded");
    };
    assert!(
        record.baseline_commit_id == "b".repeat(64) || record.baseline_commit_id == "c".repeat(64)
    );
}

#[test]
fn invalid_record_is_ignored_and_unreview_is_idempotent() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let path = b"src/lib.rs".to_vec();
    let baseline = "b".repeat(64);
    store.mark(&fixture.change, &path, &baseline).unwrap();
    let target = store.record_path(&fixture.change, &path);
    fs::write(&target, b"{broken").unwrap();

    assert_eq!(
        store.load(&fixture.change, &path).unwrap(),
        LoadResult::Unreviewed
    );
    assert!(target.exists());
    store.unreview(&fixture.change, &path).unwrap();
    assert!(!target.exists());
    store.unreview(&fixture.change, &path).unwrap();
}

#[test]
fn abandoned_temporary_file_does_not_replace_a_record() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let path = b"src/lib.rs".to_vec();
    let baseline = "b".repeat(64);
    store.mark(&fixture.change, &path, &baseline).unwrap();
    let target = store.record_path(&fixture.change, &path);
    fs::write(target.parent().unwrap().join(".tmp-dead"), b"partial").unwrap();

    let LoadResult::Reviewed(record) = store.load(&fixture.change, &path).unwrap() else {
        panic!("record was not loaded");
    };
    assert_eq!(record.baseline_commit_id, baseline);
}

#[test]
fn records_and_directories_are_user_only() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let path = b"src/lib.rs".to_vec();
    store.mark(&fixture.change, &path, &"b".repeat(64)).unwrap();
    let target = store.record_path(&fixture.change, &path);

    assert_eq!(
        fs::metadata(&target).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(target.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
}
