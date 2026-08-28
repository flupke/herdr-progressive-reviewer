use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;

use super::Fixture;
use crate::{Error, LoadResult, ReviewStore, StateKey};

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

#[test]
fn checkpoint_keys_and_paths_reject_unsafe_values() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let valid_commit = "b".repeat(64);

    for review_unit in ["", "UPPER", "change-id"] {
        assert!(matches!(
            store.mark(&review_unit.into(), b"src/lib.rs", &valid_commit),
            Err(Error::InvalidStateKey {
                field: "review unit"
            })
        ));
    }
    for commit_id in ["", "ABCDEF", "not-hex"] {
        assert!(matches!(
            store.mark(&fixture.change, b"src/lib.rs", commit_id),
            Err(Error::InvalidStateKey { field: "commit ID" })
        ));
    }
    for path in [
        &b""[..],
        &b"/absolute"[..],
        &b"src//lib.rs"[..],
        &b"src/./lib.rs"[..],
        &b"src/../lib.rs"[..],
    ] {
        assert!(matches!(
            store.mark(&fixture.change, path, &valid_commit),
            Err(Error::InvalidStateKey { field: "path" })
        ));
        assert!(matches!(
            store.load(&fixture.change, path),
            Err(Error::InvalidStateKey { field: "path" })
        ));
        assert!(matches!(
            store.unreview(&fixture.change, path),
            Err(Error::InvalidStateKey { field: "path" })
        ));
    }
}

#[test]
fn stored_checkpoint_identity_must_match_the_requested_record() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let path = b"src/lib.rs";
    store.mark(&fixture.change, path, &"b".repeat(64)).unwrap();
    let target = store.record_path(&fixture.change, path);
    let original: serde_json::Value = serde_json::from_slice(&fs::read(&target).unwrap()).unwrap();

    for (field, value) in [
        ("review_unit", serde_json::json!("c".repeat(64))),
        ("path", serde_json::json!("other.rs")),
        ("baseline_commit_id", serde_json::json!("ABCDEF")),
    ] {
        let mut altered = original.clone();
        altered[field] = value;
        fs::write(&target, serde_json::to_vec(&altered).unwrap()).unwrap();
        assert_eq!(
            store.load(&fixture.change, path).unwrap(),
            LoadResult::Unreviewed,
            "field {field}"
        );
    }
}

#[test]
fn checkpoint_records_write_review_units_and_read_the_previous_field_name() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let path = b"src/lib.rs";
    store.mark(&fixture.change, path, &"b".repeat(64)).unwrap();
    let target = store.record_path(&fixture.change, path);
    let mut stored: serde_json::Value =
        serde_json::from_slice(&fs::read(&target).unwrap()).unwrap();

    assert_eq!(stored["review_unit"], fixture.change.as_str());
    assert!(stored.get("change_id").is_none());

    stored["change_id"] = stored["review_unit"].take();
    stored.as_object_mut().unwrap().remove("review_unit");
    fs::write(&target, serde_json::to_vec(&stored).unwrap()).unwrap();

    assert!(matches!(
        store.load(&fixture.change, path).unwrap(),
        LoadResult::Reviewed(_)
    ));
}

#[test]
fn writing_a_record_rejects_a_hash_collision_with_another_path() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let path = b"src/lib.rs";
    store.mark(&fixture.change, path, &"b".repeat(64)).unwrap();
    let target = store.record_path(&fixture.change, path);
    let mut stored: serde_json::Value =
        serde_json::from_slice(&fs::read(&target).unwrap()).unwrap();
    stored["path"] = serde_json::json!("src/other.rs");
    fs::write(&target, serde_json::to_vec(&stored).unwrap()).unwrap();

    assert!(matches!(
        store.mark(&fixture.change, path, &"c".repeat(64)),
        Err(Error::StateCollision { path: collision }) if collision == target
    ));
}

#[test]
fn review_timestamp_is_an_rfc3339_value() {
    let fixture = Fixture::new();
    let record = fixture
        .store()
        .mark(&fixture.change, b"src/lib.rs", &"b".repeat(64))
        .unwrap();

    assert!(record.reviewed_at.contains('T'));
    assert!(record.reviewed_at.ends_with('Z'));
}

#[test]
fn unreview_reports_a_non_file_target() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let path = b"src/lib.rs";
    let target = store.record_path(&fixture.change, path);
    fs::create_dir_all(&target).unwrap();

    assert!(matches!(
        store.unreview(&fixture.change, path),
        Err(Error::StateIo { .. })
    ));
}
