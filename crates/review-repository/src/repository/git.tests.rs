use crate::repository::{ChangeKind, ChangedFile, FileKind};

#[test]
fn git_type_change_status_is_preserved() {
    let [file] = ChangedFile::parse_git(b":100644 100644 old new T\0file\0")
        .unwrap()
        .try_into()
        .unwrap();

    assert_eq!(file.change, ChangeKind::TypeChanged);
}

#[test]
fn git_mode_change_overrides_modified_status() {
    let [file] = ChangedFile::parse_git(b":100644 120000 old new M\0file\0")
        .unwrap()
        .try_into()
        .unwrap();

    assert_eq!(file.change, ChangeKind::TypeChanged);
}

#[test]
fn git_submodule_mode_is_parsed_as_a_gitlink() {
    let [file] = ChangedFile::parse_git(b":000000 160000 old new A\0module\0")
        .unwrap()
        .try_into()
        .unwrap();

    assert_eq!(file.old_kind, FileKind::Absent);
    assert_eq!(file.new_kind, FileKind::Gitlink);
    assert_eq!(file.change, ChangeKind::Added);
}
