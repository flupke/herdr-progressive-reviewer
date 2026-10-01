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
