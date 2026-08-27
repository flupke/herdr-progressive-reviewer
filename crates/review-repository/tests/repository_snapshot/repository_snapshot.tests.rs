use super::*;

#[test_case::test_case(RepoType::Git; "git")]
#[test_case::test_case(RepoType::Jj; "jj")]
fn reads_current_and_base_file_content(repository_type: RepoType) {
    let context = RepositoryTestContext::new(repository_type);
    context
        .repository_files
        .write("file.txt", b"base content\n");
    context.repository_files.new_change("edit file");
    context
        .repository_files
        .write("file.txt", b"current content\n");
    let snapshot = complete_repository_snapshot(&context.repository);
    let path = snapshot.files[0].review_path();

    assert_eq!(
        context
            .repository
            .file_at(snapshot.identity.snapshot_id(), path)
            .unwrap(),
        b"current content\n"
    );
    assert_eq!(
        context.repository.base_file_at(&snapshot, path).unwrap(),
        b"base content\n"
    );
}

#[test_case::test_case(RepoType::Git; "git")]
#[test_case::test_case(RepoType::Jj; "jj")]
fn renamed_and_edited_files_diff_both_paths(repository_type: RepoType) {
    let context = RepositoryTestContext::new(repository_type);
    context
        .repository_files
        .write("old.txt", b"one\nbefore\nthree\n");
    context.repository_files.new_change("rename and edit");
    context.repository_files.rename("old.txt", "new.txt");
    context
        .repository_files
        .write("new.txt", b"one\nafter\nthree\n");
    let snapshot = complete_repository_snapshot(&context.repository);
    let file = &snapshot.files[0];
    let diff = String::from_utf8(context.repository.diff(&snapshot, file).unwrap()).unwrap();

    assert!(diff.contains("-before"), "{diff}");
    assert!(diff.contains("+after"), "{diff}");
}
