use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;

use review_hunks::{Attribution, AuthoredLines, Reviewed};
use review_types::MarkAuthor;

use super::Fixture;
use crate::{Error, LoadResult, PartialReview, ReviewStore, StateKey};

#[test]
fn paths_round_trip_and_keys_are_stable() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let baseline = "b".repeat(64);

    for path in [b"src/lib.rs".to_vec(), b"invalid-\xff".to_vec()] {
        store
            .mark(&fixture.change, &path, &baseline, &MarkAuthor::Reviewer)
            .unwrap();
        let LoadResult::Reviewed(record) = store.load(&fixture.change, &path).unwrap() else {
            panic!("record was not loaded");
        };
        assert_eq!(record.path, path);
        assert_eq!(record.baseline_commit_id, baseline);
    }

    assert_eq!(
        StateKey::hash(b"src/lib.rs").0,
        "b1a35a68f14e696205874893c07fd24fdb88882b47c23cc0e0c80a30c7d53759"
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

    first
        .mark(&fixture.change, &left, &baseline, &MarkAuthor::Reviewer)
        .unwrap();
    first
        .mark(&fixture.change, &right, &baseline, &MarkAuthor::Reviewer)
        .unwrap();

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
        .mark(
            &fixture.change,
            &path,
            &"d".repeat(64),
            &MarkAuthor::Reviewer,
        )
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
                store
                    .mark(&change, &path, &baseline, &MarkAuthor::Reviewer)
                    .unwrap();
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
    store
        .mark(&fixture.change, &path, &baseline, &MarkAuthor::Reviewer)
        .unwrap();
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
    store
        .mark(&fixture.change, &path, &baseline, &MarkAuthor::Reviewer)
        .unwrap();
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
    store
        .mark(
            &fixture.change,
            &path,
            &"b".repeat(64),
            &MarkAuthor::Reviewer,
        )
        .unwrap();
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
            store.mark(
                &review_unit.into(),
                b"src/lib.rs",
                &valid_commit,
                &MarkAuthor::Reviewer
            ),
            Err(Error::InvalidStateKey {
                field: "review unit"
            })
        ));
    }
    for commit_id in ["", "ABCDEF", "not-hex"] {
        assert!(matches!(
            store.mark(
                &fixture.change,
                b"src/lib.rs",
                commit_id,
                &MarkAuthor::Reviewer
            ),
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
            store.mark(&fixture.change, path, &valid_commit, &MarkAuthor::Reviewer),
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
    store
        .mark(
            &fixture.change,
            path,
            &"b".repeat(64),
            &MarkAuthor::Reviewer,
        )
        .unwrap();
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
    store
        .mark(
            &fixture.change,
            path,
            &"b".repeat(64),
            &MarkAuthor::Reviewer,
        )
        .unwrap();
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
    store
        .mark(
            &fixture.change,
            path,
            &"b".repeat(64),
            &MarkAuthor::Reviewer,
        )
        .unwrap();
    let target = store.record_path(&fixture.change, path);
    let mut stored: serde_json::Value =
        serde_json::from_slice(&fs::read(&target).unwrap()).unwrap();
    stored["path"] = serde_json::json!("src/other.rs");
    fs::write(&target, serde_json::to_vec(&stored).unwrap()).unwrap();

    assert!(matches!(
        store.mark(&fixture.change, path, &"c".repeat(64), &MarkAuthor::Reviewer),
        Err(Error::StateCollision { path: collision }) if collision == target
    ));
}

#[test]
fn review_timestamp_is_an_rfc3339_value() {
    let fixture = Fixture::new();
    let record = fixture
        .store()
        .mark(
            &fixture.change,
            b"src/lib.rs",
            &"b".repeat(64),
            &MarkAuthor::Reviewer,
        )
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

fn partial() -> PartialReview {
    PartialReview {
        base: b"a\nb\n".to_vec(),
        reviewed: Reviewed {
            text: b"a\nB\n\xff".to_vec(),
            attribution: Attribution {
                default: MarkAuthor::Reviewer,
                removed: Vec::new(),
                added: vec![AuthoredLines {
                    lines: 2..3,
                    author: MarkAuthor::Jev,
                }],
            },
        },
    }
}

#[test]
fn hunk_marks_keep_the_reviewed_version_and_its_base() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let path = b"src/lib.rs".to_vec();

    store
        .mark_partial(&fixture.change, &path, &"b".repeat(64), partial())
        .unwrap();

    let LoadResult::Reviewed(record) = store.load(&fixture.change, &path).unwrap() else {
        panic!("record was not loaded");
    };
    assert_eq!(record.partial, Some(Box::new(partial())));
    let stored: serde_json::Value =
        serde_json::from_slice(&fs::read(store.record_path(&fixture.change, &path)).unwrap())
            .unwrap();
    assert_eq!(stored["schema_version"], 2);
}

#[test]
fn whole_file_marks_keep_their_author() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let path = b"src/lib.rs".to_vec();

    store
        .mark(&fixture.change, &path, &"b".repeat(64), &MarkAuthor::Jev)
        .unwrap();

    let LoadResult::Reviewed(record) = store.load(&fixture.change, &path).unwrap() else {
        panic!("record was not loaded");
    };
    assert_eq!(record.author, MarkAuthor::Jev);
}

#[test]
fn marks_written_before_authors_were_stored_are_the_reviewers() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let path = b"src/lib.rs".to_vec();
    store
        .mark_partial(&fixture.change, &path, &"b".repeat(64), partial())
        .unwrap();
    let target = store.record_path(&fixture.change, &path);
    let mut stored: serde_json::Value =
        serde_json::from_slice(&fs::read(&target).unwrap()).unwrap();
    stored.as_object_mut().unwrap().remove("author");
    stored["partial"]
        .as_object_mut()
        .unwrap()
        .remove("attribution");
    fs::write(&target, serde_json::to_vec(&stored).unwrap()).unwrap();

    let LoadResult::Reviewed(record) = store.load(&fixture.change, &path).unwrap() else {
        panic!("record was not loaded");
    };
    assert_eq!(record.author, MarkAuthor::Reviewer);
    assert_eq!(
        record.partial.unwrap().reviewed.attribution,
        Attribution::uniform(MarkAuthor::Reviewer)
    );
}

#[test]
fn a_whole_file_mark_replaces_a_hunk_mark() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let path = b"src/lib.rs".to_vec();
    store
        .mark_partial(&fixture.change, &path, &"b".repeat(64), partial())
        .unwrap();

    store
        .mark(
            &fixture.change,
            &path,
            &"c".repeat(64),
            &MarkAuthor::Reviewer,
        )
        .unwrap();

    let LoadResult::Reviewed(record) = store.load(&fixture.change, &path).unwrap() else {
        panic!("record was not loaded");
    };
    assert_eq!(record.partial, None);
    assert_eq!(record.baseline_commit_id, "c".repeat(64));
}

#[test]
fn a_hunk_mark_without_its_versions_is_ignored() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let path = b"src/lib.rs".to_vec();
    store
        .mark_partial(&fixture.change, &path, &"b".repeat(64), partial())
        .unwrap();
    let target = store.record_path(&fixture.change, &path);
    let mut stored: serde_json::Value =
        serde_json::from_slice(&fs::read(&target).unwrap()).unwrap();
    stored.as_object_mut().unwrap().remove("partial");
    fs::write(&target, serde_json::to_vec(&stored).unwrap()).unwrap();

    assert_eq!(
        store.load(&fixture.change, &path).unwrap(),
        LoadResult::Unreviewed
    );
}

#[test]
fn hunk_marks_of_large_files_round_trip() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let path = b"src/lib.rs".to_vec();
    let large = PartialReview {
        base: vec![b'a'; 2 * 1024 * 1024],
        reviewed: Reviewed {
            text: vec![b'b'; 2 * 1024 * 1024],
            attribution: Attribution::default(),
        },
    };

    store
        .mark_partial(&fixture.change, &path, &"c".repeat(64), large.clone())
        .unwrap();

    let LoadResult::Reviewed(record) = store.load(&fixture.change, &path).unwrap() else {
        panic!("record was not loaded");
    };
    assert_eq!(record.partial, Some(Box::new(large)));
}

#[test]
fn a_mark_by_an_author_this_reviewer_does_not_know_is_kept_unknown() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let path = b"src/lib.rs".to_vec();
    store
        .mark_partial(&fixture.change, &path, &"b".repeat(64), partial())
        .unwrap();
    let target = store.record_path(&fixture.change, &path);
    let mut stored: serde_json::Value =
        serde_json::from_slice(&fs::read(&target).unwrap()).unwrap();
    stored["partial"]["attribution"]["added"][0]["author"] =
        serde_json::json!({ "kind": "someone_new" });
    fs::write(&target, serde_json::to_vec(&stored).unwrap()).unwrap();

    assert_eq!(
        store.load(&fixture.change, &path).unwrap(),
        LoadResult::UnknownSchema
    );
}

#[test]
fn overlapping_stored_authors_give_each_line_one_author() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let path = b"src/lib.rs".to_vec();
    store
        .mark_partial(&fixture.change, &path, &"b".repeat(64), partial())
        .unwrap();
    let target = store.record_path(&fixture.change, &path);
    let mut stored: serde_json::Value =
        serde_json::from_slice(&fs::read(&target).unwrap()).unwrap();
    stored["partial"]["attribution"]["added"] = serde_json::json!([
        { "start": 3, "end": 5, "author": { "kind": "reviewer" } },
        { "start": 0, "end": 4, "author": { "kind": "jev" } },
    ]);
    fs::write(&target, serde_json::to_vec(&stored).unwrap()).unwrap();

    let LoadResult::Reviewed(record) = store.load(&fixture.change, &path).unwrap() else {
        panic!("record was not loaded");
    };
    assert_eq!(
        record.partial.unwrap().reviewed.attribution.added,
        [AuthoredLines {
            lines: 0..4,
            author: MarkAuthor::Jev,
        }]
    );
}
