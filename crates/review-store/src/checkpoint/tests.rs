use super::*;

#[test]
fn clearing_all_marks_is_scoped_to_file_records_of_one_review() {
    let state = tempfile::tempdir().unwrap();
    let repository = tempfile::tempdir().unwrap();
    let store = ReviewStore::open(state.path(), repository.path()).unwrap();
    let unit = "current".into();
    for path in [b"present.rs".as_slice(), b"no-longer-present.rs"] {
        store
            .mark(&unit, path, "aabb", &MarkAuthor::Reviewer)
            .unwrap();
    }
    let other = store
        .mark(
            &"another".into(),
            b"present.rs",
            "ccdd",
            &MarkAuthor::Reviewer,
        )
        .unwrap();
    let discussion = store.repository_dir.join("changes/current/discussion.json");
    fs::write(&discussion, "retained discussion").unwrap();
    store.save_file_pane_width(42).unwrap();

    store.unreview_all(&unit).unwrap();
    store.unreview_all(&unit).unwrap();

    for path in [b"present.rs".as_slice(), b"no-longer-present.rs"] {
        assert_eq!(store.load(&unit, path).unwrap(), LoadResult::Unreviewed);
    }
    assert_eq!(
        store.load(&"another".into(), b"present.rs").unwrap(),
        LoadResult::Reviewed(other)
    );
    assert_eq!(
        fs::read_to_string(discussion).unwrap(),
        "retained discussion"
    );
    assert_eq!(store.file_pane_width().unwrap(), Some(42));
    assert!(store.unreview_all(&"../another".into()).is_err());
    store
        .mark(&unit, b"present.rs", "eeff", &MarkAuthor::Reviewer)
        .unwrap();
    assert!(matches!(
        store.load(&unit, b"present.rs").unwrap(),
        LoadResult::Reviewed(_)
    ));
}

#[test]
fn an_expired_mark_is_removed_but_a_mark_written_since_stays() {
    let state = tempfile::tempdir().unwrap();
    let repository = tempfile::tempdir().unwrap();
    let store = ReviewStore::open(state.path(), repository.path()).unwrap();
    let unit = "current".into();
    let expired = store
        .mark(
            &unit,
            b"a.rs",
            &"a".repeat(40),
            &review_types::MarkAuthor::Reviewer,
        )
        .unwrap();

    store
        .unreview_expired(&unit, b"a.rs", "2000-01-01T00:00:00Z")
        .unwrap();
    assert!(matches!(
        store.load(&unit, b"a.rs").unwrap(),
        LoadResult::Reviewed(_)
    ));
    store
        .unreview_expired(&unit, b"a.rs", &expired.reviewed_at)
        .unwrap();
    assert_eq!(store.load(&unit, b"a.rs").unwrap(), LoadResult::Unreviewed);
}
