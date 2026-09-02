use std::path::Path;

use super::{
    Cancellation, ChangeKind, ChangedFile, FileKind, RepoPath, RepoType, RepositoryProcess,
    SnapshotIdentity,
};
use crate::Error;

#[test]
fn repository_type_converts_to_and_from_lowercase_text() {
    assert_eq!(RepoType::Git.to_string(), "git");
    assert_eq!(RepoType::Jj.to_string(), "jj");
    assert_eq!("git".parse(), Ok(RepoType::Git));
    assert_eq!("jj".parse(), Ok(RepoType::Jj));
    assert!("unknown".parse::<RepoType>().is_err());
}

#[test]
fn repository_paths_preserve_non_utf8_bytes() {
    let path = RepoPath::from_bytes(b"invalid-\xff.txt");

    assert_eq!(path.0, b"invalid-\xff.txt");
    assert_eq!(path.display(), r"invalid-\xff.txt");
}

#[test]
fn cancellation_stops_a_child_command() {
    let cancellation = Cancellation::default();
    cancellation.cancel();

    let error = RepositoryProcess::new("jj", Path::new("."), "test jj cancellation", &cancellation)
        .output(["version"])
        .unwrap_err();

    assert!(matches!(error, Error::CommandCancelled { .. }));
}

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

#[test]
fn jj_file_records_require_complete_groups_and_terminators() {
    let output = b"z-old\0z-new\0file\0file\0modified\0a-old\0a-new\0file\0file\0renamed\0";
    let files = ChangedFile::parse_all(output).unwrap();

    assert_eq!(files.len(), 2);
    assert_eq!(files[0].display_path, "a-old => a-new");
    assert_eq!(files[1].display_path, "z-old => z-new");
    assert!(ChangedFile::parse_all(&output[..output.len() - 1]).is_err());
    assert!(ChangedFile::parse_all(b"path\0path\0file\0file\0").is_err());
}

#[test]
fn jj_statistics_require_complete_groups_and_update_matching_files() {
    let mut files = ChangedFile::parse_all(b"file\0file\0file\0file\0modified\0").unwrap();

    ChangedFile::add_stats(&mut files, b"file\0\x31\x32\0\x33\0").unwrap();

    assert_eq!(files[0].lines_added, 12);
    assert_eq!(files[0].lines_removed, 3);
    assert!(ChangedFile::add_stats(&mut files, b"file\0\x31\x32\0").is_err());
    assert!(ChangedFile::add_stats(&mut files, b"file\0not-a-number\0\x33\0").is_err());
}

#[test]
fn rename_diff_paths_include_each_distinct_side_once() {
    let renamed = ChangedFile::parse_all(b"old.rs\0new.rs\0file\0file\0renamed\0")
        .unwrap()
        .pop()
        .unwrap();
    let unchanged = ChangedFile::parse_all(b"same.rs\0same.rs\0file\0file\0modified\0")
        .unwrap()
        .pop()
        .unwrap();

    assert_eq!(
        renamed
            .diff_paths()
            .map(RepoPath::as_bytes)
            .collect::<Vec<_>>(),
        [b"old.rs".as_slice(), b"new.rs".as_slice()]
    );
    assert_eq!(
        unchanged
            .diff_paths()
            .map(RepoPath::as_bytes)
            .collect::<Vec<_>>(),
        [b"same.rs".as_slice()]
    );
}

#[test]
fn jj_snapshot_identity_requires_two_nonempty_ids_and_a_terminator() {
    assert_eq!(
        SnapshotIdentity::parse(b"change\0commit\0description\0").unwrap(),
        SnapshotIdentity::Jj {
            change_id: super::ChangeId("change".into()),
            snapshot_id: super::SnapshotId("commit".to_owned()),
            description: "description".to_owned(),
        }
    );
    for invalid in [
        &b"\0commit\0description\0"[..],
        &b"change\0\0description\0"[..],
        &b"change\0commit\0description"[..],
        &b"change\0commit\0description\0extra"[..],
    ] {
        assert!(SnapshotIdentity::parse(invalid).is_err(), "{invalid:?}");
    }
}
