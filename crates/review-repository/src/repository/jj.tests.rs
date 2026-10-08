use review_test_support::{JjFixture, JjLayout};

use super::{JjBackend, parse_revision_candidates, parse_revision_history};
use crate::repository::{
    ChangeId, ChangeKind, ChangedFile, MetadataScope, MetadataWatch, Repository, RepositoryBackend,
    RevisionDirection, SnapshotId, SnapshotIdentity,
};

#[test]
fn revision_candidate_records_preserve_graph_order() {
    let candidates =
        parse_revision_candidates(b"first-full\0first\0First description\0second-full\0second\0\0")
            .unwrap();

    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].change_id.as_str(), "first-full");
    assert_eq!(candidates[0].short_change_id, "first");
    assert_eq!(candidates[0].description, "First description");
    assert_eq!(candidates[1].change_id.as_str(), "second-full");
    assert_eq!(candidates[1].description, "");
}

#[test]
fn revision_candidate_records_require_complete_utf8_groups() {
    assert!(parse_revision_candidates(b"change\0short\0description").is_err());
    assert!(parse_revision_candidates(b"change\0short\0\0").is_ok());
    assert!(parse_revision_candidates(b"change\0short\0description\0extra\0").is_err());
    assert!(parse_revision_candidates(b"change\0short\0\xff\0").is_err());
}

#[test]
fn revision_history_records_keep_graph_text_and_extract_change_ids() {
    let history = parse_revision_history(
        b"\x1b[32m@\x1b[0m  \x1e\x1b[35mfull-id\x1b[39m:commit-id:short:1:0\x1f\x1d\x1b[35mshort\x1b[39m message\n\xe2\x94\x82\n",
    )
    .unwrap();

    assert_eq!(history.len(), 2);
    assert_eq!(history[0].change_id.as_ref().unwrap().as_str(), "full-id");
    assert_eq!(history[0].short_change_id.as_deref(), Some("short"));
    assert_eq!(
        history[0].commit_id.as_ref().map(SnapshotId::as_str),
        Some("commit-id")
    );
    assert_eq!(history[0].plain_text, "@  short message");
    assert!(history[0].is_current);
    assert!(!history[0].is_immutable);
    assert!(history[0].text.contains("\x1b[32m@"));
    let graph_end = history[0].graph_end.unwrap();
    assert_eq!(&history[0].text[..graph_end], "\x1b[32m@\x1b[0m  ");
    assert!(history[0].text[graph_end..].contains("short\x1b[39m message"));
    assert_eq!(history[1].graph_end, None);
    assert_eq!(history[1].change_id, None);
    assert_eq!(history[1].short_change_id, None);
    assert_eq!(history[1].plain_text, "│");
    assert!(!history[1].is_current);
    assert!(!history[1].is_immutable);
}

#[test]
fn revision_candidates_and_edits_follow_the_real_jj_graph() {
    let fixture = JjFixture::new(JjLayout::NonColocated);
    let repository = Repository::discover(fixture.root()).unwrap();
    let parent = fixture.change_id();

    fixture.new_change("first child");
    let first_child = fixture.change_id();
    fixture.edit(&parent);
    fixture.new_change("second child");
    let second_child = fixture.change_id();
    fixture.edit(&parent);

    let children = repository
        .revision_candidates(RevisionDirection::Children)
        .unwrap();
    assert_eq!(children.len(), 2);
    assert!(children.iter().any(|child| {
        child.change_id.as_str() == first_child && child.description == "first child"
    }));
    assert!(children.iter().any(|child| {
        child.change_id.as_str() == second_child && child.description == "second child"
    }));

    let first_child = children
        .iter()
        .find(|child| child.change_id.as_str() == first_child)
        .unwrap();
    assert!(repository.edit_revision(&first_child.change_id).unwrap());
    assert_eq!(fixture.change_id(), first_child.change_id.as_str());

    let parents = repository
        .revision_candidates(RevisionDirection::Parents)
        .unwrap();
    assert_eq!(parents.len(), 1);
    assert_eq!(parents[0].change_id.as_str(), parent);
}

#[test]
fn revision_history_uses_the_real_jj_graph_and_stops_at_the_immutable_boundary() {
    let fixture = JjFixture::new(JjLayout::NonColocated);
    let repository = Repository::discover(fixture.root()).unwrap();
    fixture.new_change("first mutable change");
    let first = fixture.change_id();
    fixture.new_change("second mutable change");
    let second = fixture.change_id();

    let history = repository.revision_history().unwrap();
    let change_ids = history
        .iter()
        .filter_map(|line| line.change_id.as_ref().map(ChangeId::as_str))
        .collect::<Vec<_>>();

    assert!(change_ids.contains(&second.as_str()));
    assert!(change_ids.contains(&first.as_str()));
    assert!(history.iter().any(|line| line.is_immutable));
    assert!(
        history
            .iter()
            .filter(|line| {
                line.change_id.as_ref().is_some_and(|change_id| {
                    change_id.as_str() == first || change_id.as_str() == second
                })
            })
            .all(|line| !line.is_immutable)
    );
    assert!(
        history
            .iter()
            .any(|line| line.plain_text.contains("first mutable change"))
    );
    assert!(history.len() >= 3);
    assert_eq!(
        history
            .iter()
            .find(|line| line.is_current)
            .unwrap()
            .change_id,
        Some(ChangeId::from(second.clone()))
    );
}

#[test]
fn revision_history_includes_mutable_children_after_editing_an_ancestor() {
    let fixture = JjFixture::new(JjLayout::NonColocated);
    let repository = Repository::discover(fixture.root()).unwrap();
    fixture.new_change("mutable parent");
    let mutable_parent = fixture.change_id();
    fixture.new_change("mutable child");
    let mutable_child = fixture.change_id();
    fixture.jj(["edit", &mutable_parent]);

    let history = repository.revision_history().unwrap();

    assert!(history.iter().any(|line| {
        !line.is_immutable
            && line
                .change_id
                .as_ref()
                .is_some_and(|change_id| change_id.as_str() == mutable_child)
    }));
}

#[test]
fn revision_history_includes_immutable_children_of_the_boundary() {
    let fixture = JjFixture::new(JjLayout::NonColocated);
    let repository = Repository::discover(fixture.root()).unwrap();
    fixture.jj(["new", "root()", "-m", "first immutable child"]);
    let first_immutable_child = fixture.change_id();
    fixture.jj(["bookmark", "create", "first-immutable-child"]);
    fixture.jj(["new", "root()", "-m", "second immutable child"]);
    let second_immutable_child = fixture.change_id();
    fixture.jj(["bookmark", "create", "second-immutable-child"]);
    fixture.jj([
        "config",
        "set",
        "--repo",
        "revset-aliases.'immutable_heads()'",
        "first-immutable-child | second-immutable-child",
    ]);
    fixture.jj(["new", "root()", "-m", "current mutable change"]);

    let history = repository.revision_history().unwrap();

    for immutable_child in [first_immutable_child, second_immutable_child] {
        assert!(history.iter().any(|line| {
            line.is_immutable
                && line
                    .change_id
                    .as_ref()
                    .is_some_and(|change_id| change_id.as_str() == immutable_child)
        }));
    }
}

#[test]
fn jj_file_records_require_complete_groups_and_terminators() {
    let output = b"z-old\0z-new\0file\0file\0modified\0a-old\0a-new\0file\0file\0renamed\0";
    let files = ChangedFile::parse_jj(output).unwrap();

    assert_eq!(files.len(), 2);
    assert_eq!(files[0].display_path, "a-old => a-new");
    assert_eq!(files[1].display_path, "z-old => z-new");
    assert!(ChangedFile::parse_jj(&output[..output.len() - 1]).is_err());
    assert!(ChangedFile::parse_jj(b"path\0path\0file\0file\0").is_err());
}

#[test]
fn resolved_jj_conflict_is_a_modified_file() {
    let file = ChangedFile::parse_jj(b"file\0file\0conflict\0file\0modified\0")
        .unwrap()
        .pop()
        .unwrap();

    assert_eq!(file.change, ChangeKind::Modified);
}

#[test]
fn jj_statistics_require_complete_groups_and_update_matching_files() {
    let mut files = ChangedFile::parse_jj(b"file\0file\0file\0file\0modified\0").unwrap();

    ChangedFile::add_jj_stats(&mut files, b"file\0\x31\x32\0\x33\0").unwrap();

    assert_eq!(files[0].statistics.lines_added, 12);
    assert_eq!(files[0].statistics.lines_removed, 3);
    assert!(ChangedFile::add_jj_stats(&mut files, b"file\0\x31\x32\0").is_err());
    assert!(ChangedFile::add_jj_stats(&mut files, b"file\0not-a-number\0\x33\0").is_err());
}

#[test]
fn jj_snapshot_identity_requires_two_nonempty_ids_and_a_terminator() {
    assert_eq!(
        SnapshotIdentity::parse_jj(b"change\0commit\0description\0short\0").unwrap(),
        SnapshotIdentity::Jj {
            change_id: ChangeId::from("change".to_owned()),
            snapshot_id: SnapshotId::from("commit".to_owned()),
            description: "description".to_owned(),
            display_id: "short".to_owned(),
        }
    );
    for invalid in [
        &b"\0commit\0description\0"[..],
        &b"change\0\0description\0"[..],
        &b"change\0commit\0description"[..],
        &b"change\0commit\0description\0extra"[..],
    ] {
        assert!(SnapshotIdentity::parse_jj(invalid).is_err(), "{invalid:?}");
    }
}

#[test]
fn watch_plan_falls_back_to_git_metadata_without_operation_heads() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    std::fs::create_dir_all(root.join(".git/refs")).unwrap();
    std::fs::create_dir(root.join(".jj")).unwrap();
    let git = std::fs::canonicalize(root.join(".git")).unwrap();

    let plan = JjBackend::default().watch_plan(root);

    assert_eq!(
        plan.metadata,
        [
            MetadataWatch {
                directory: git.clone(),
                scope: MetadataScope::Entries,
            },
            MetadataWatch {
                directory: git.join("refs"),
                scope: MetadataScope::Subtree,
            },
        ]
    );
    assert_eq!(plan.git_excludes, None);
}

#[test]
fn a_snapshot_of_another_revision_reads_its_own_files_and_leaves_the_working_copy() {
    let fixture = JjFixture::new(JjLayout::NonColocated);
    let repository = Repository::discover(fixture.root()).unwrap();
    fixture.new_change("first");
    fixture.write("first.txt", b"one\ntwo\n");
    let first = fixture.change_id();
    fixture.new_change("second");
    fixture.write("second.txt", b"three\n");
    let second = fixture.change_id();

    let snapshot = repository
        .snapshot_of(&ChangeId::from(first.clone()))
        .unwrap()
        .unwrap();

    assert_eq!(snapshot.identity.review_unit().as_str(), first);
    let paths = snapshot
        .files
        .iter()
        .map(|file| file.display_path.as_str())
        .collect::<Vec<_>>();
    assert_eq!(paths, ["first.txt"]);
    assert_eq!(snapshot.files[0].statistics.lines_added, 2);
    // The working copy stays on its own change.
    assert_eq!(fixture.change_id(), second);
    let second_snapshot = repository
        .snapshot_of(&ChangeId::from(second))
        .unwrap()
        .unwrap();
    assert_eq!(second_snapshot.files.len(), 1);
}
