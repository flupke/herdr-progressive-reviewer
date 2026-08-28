use review_repository::repository::{RevisionCandidate, RevisionDirection};
use review_state::ReviewStatus;

use super::*;
use crate::app::{Message, ReviewFile};

fn loaded_app() -> ReviewApp {
    let mut app = ReviewApp::default();
    let _ = app.update(Message::FilesLoaded {
        review_unit: "current-change".into(),
        commit_id: "current-commit".to_owned(),
        description: String::new(),
        files: vec![ReviewFile::new("src/lib.rs", ReviewStatus::Unreviewed)],
    });
    app
}

fn candidate(id: &str, description: &str) -> RevisionCandidate {
    RevisionCandidate {
        change_id: review_repository::repository::ChangeId::from(id.to_owned()),
        short_change_id: id.to_owned(),
        description: description.to_owned(),
    }
}

#[test]
fn one_revision_candidate_is_edited_without_a_selector() {
    let mut app = loaded_app();

    assert_eq!(
        app.start_revision_navigation(RevisionDirection::Parents),
        Action::LoadRevisionCandidates(RevisionDirection::Parents)
    );
    assert_eq!(
        app.load_revision_candidates(
            RevisionDirection::Parents,
            Ok(vec![candidate("parent", "Parent")]),
        ),
        Action::EditRevision {
            change_id: review_repository::repository::ChangeId::from("parent".to_owned()),
        }
    );
    assert!(matches!(
        app.revision_navigation,
        Some(RevisionNavigationState::Editing { .. })
    ));
    assert_eq!(app.active_popup, None);
}

#[test]
fn selector_edits_the_chosen_revision_and_can_cancel() {
    let mut app = loaded_app();
    let _ = app.start_revision_navigation(RevisionDirection::Children);
    let _ = app.load_revision_candidates(
        RevisionDirection::Children,
        Ok(vec![
            candidate("first", "First"),
            candidate("second", "Second"),
        ]),
    );

    assert_eq!(app.active_popup, Some(ActivePopup::RevisionSelector));
    assert_eq!(app.revision_selector_key(Key::Char('j')), Action::None);
    assert_eq!(
        app.revision_selector_key(Key::Enter),
        Action::EditRevision {
            change_id: review_repository::repository::ChangeId::from("second".to_owned()),
        }
    );

    app.fail_revision_edit(None);
    let _ = app.start_revision_navigation(RevisionDirection::Children);
    let _ = app.load_revision_candidates(
        RevisionDirection::Children,
        Ok(vec![
            candidate("first", "First"),
            candidate("second", "Second"),
        ]),
    );
    assert_eq!(app.revision_selector_key(Key::Escape), Action::None);
    assert!(app.revision_navigation.is_none());
    assert_eq!(app.active_popup, None);
}

#[test]
fn revision_refresh_restores_the_file_and_records_the_commit_jump() {
    let mut app = loaded_app();
    let _ = app.start_revision_navigation(RevisionDirection::Parents);
    let _ = app.load_revision_candidates(
        RevisionDirection::Parents,
        Ok(vec![candidate("parent", "Parent")]),
    );

    let action = app.update(Message::FilesLoaded {
        review_unit: "parent".into(),
        commit_id: "parent-commit".to_owned(),
        description: "Parent".to_owned(),
        files: vec![ReviewFile::new("src/lib.rs", ReviewStatus::Unreviewed)],
    });

    assert!(matches!(action, Action::LoadDiff { .. }));
    assert!(app.revision_navigation.is_some());
    let action = app.update(Message::DiffLoaded {
        commit_id: "parent-commit".to_owned(),
        path: "src/lib.rs".to_owned(),
        rows: vec![review_repository::diff::DiffRow::Add {
            new_line: 1,
            text: "+changed".to_owned(),
        }],
        old_content: None,
        new_content: None,
    });
    assert_eq!(action, Action::None);
    assert!(app.revision_navigation.is_none());
    assert_eq!(
        app.previous_location(),
        Action::EditRevision {
            change_id: review_repository::repository::ChangeId::from("current-change".to_owned(),),
        }
    );
}

#[test]
fn empty_destination_diff_finishes_the_revision_jump() {
    let mut app = loaded_app();
    let _ = app.start_revision_navigation(RevisionDirection::Parents);
    let _ = app.load_revision_candidates(
        RevisionDirection::Parents,
        Ok(vec![candidate("parent", "Parent")]),
    );
    let _ = app.update(Message::FilesLoaded {
        review_unit: "parent".into(),
        commit_id: "parent-commit".to_owned(),
        description: "Parent".to_owned(),
        files: vec![ReviewFile::new("src/lib.rs", ReviewStatus::Unreviewed)],
    });

    let action = app.update(Message::DiffLoaded {
        commit_id: "parent-commit".to_owned(),
        path: "src/lib.rs".to_owned(),
        rows: Vec::new(),
        old_content: None,
        new_content: None,
    });

    assert_eq!(action, Action::None);
    assert!(app.revision_navigation.is_none());
}

#[test]
fn empty_source_destination_diff_finishes_the_revision_jump() {
    let mut app = loaded_app();
    let origin = app.current_review_location().unwrap();
    app.revision_navigation = Some(RevisionNavigationState::Editing {
        target_change_id: review_repository::repository::ChangeId::from(
            "current-change".to_owned(),
        ),
        destination: ReviewLocation::Source {
            review_unit: "current-change".into(),
            location: review_lsp::SourceLocation {
                path: app.repository_root.join("src/lib.rs"),
                line: 1,
                byte_column: 0,
                end_line: 1,
                end_byte_column: 1,
            },
        },
        completion: RevisionNavigationCompletion::RecordJump { origin },
    });

    let action = app.update(Message::DiffLoaded {
        commit_id: "current-commit".to_owned(),
        path: "src/lib.rs".to_owned(),
        rows: Vec::new(),
        old_content: None,
        new_content: None,
    });

    assert_eq!(action, Action::None);
    assert!(app.revision_navigation.is_none());
}

#[test]
fn revision_navigation_waits_for_a_repository_refresh() {
    let mut app = loaded_app();
    let _ = app.update(Message::RepositoryRefreshStarted);

    assert_eq!(
        app.start_revision_navigation(RevisionDirection::Parents),
        Action::None
    );

    let _ = app.update(Message::RepositoryRefreshFinished);
    assert_eq!(
        app.start_revision_navigation(RevisionDirection::Parents),
        Action::LoadRevisionCandidates(RevisionDirection::Parents)
    );
}

#[test]
fn revision_navigation_waits_for_all_overlapping_repository_refreshes() {
    let mut app = loaded_app();
    let _ = app.update(Message::RepositoryRefreshStarted);
    let _ = app.update(Message::RepositoryRefreshStarted);
    let _ = app.update(Message::RepositoryRefreshFinished);

    assert_eq!(
        app.start_revision_navigation(RevisionDirection::Parents),
        Action::None
    );

    let _ = app.update(Message::RepositoryRefreshFinished);
    assert_eq!(
        app.start_revision_navigation(RevisionDirection::Parents),
        Action::LoadRevisionCandidates(RevisionDirection::Parents)
    );
}

#[test]
fn failed_history_edit_restores_the_history_stacks() {
    let mut app = loaded_app();
    let current = app.current_review_location().unwrap();
    let parent = current.for_review_unit("parent");
    assert!(app.location_history.record_jump(parent, &current));

    assert_eq!(
        app.previous_location(),
        Action::EditRevision {
            change_id: review_repository::repository::ChangeId::from("parent".to_owned()),
        }
    );
    let _ = app.fail_revision_edit(None);

    assert_eq!(
        app.previous_location(),
        Action::EditRevision {
            change_id: review_repository::repository::ChangeId::from("parent".to_owned()),
        }
    );
}

#[test]
fn source_failure_finishes_only_the_matching_revision_restore() {
    let mut app = loaded_app();
    let origin = app.current_review_location().unwrap();
    app.revision_navigation = Some(RevisionNavigationState::Editing {
        target_change_id: review_repository::repository::ChangeId::from(
            "current-change".to_owned(),
        ),
        destination: ReviewLocation::Source {
            review_unit: "current-change".into(),
            location: review_lsp::SourceLocation {
                path: app.repository_root.join("src/other.rs"),
                line: 1,
                byte_column: 0,
                end_line: 1,
                end_byte_column: 1,
            },
        },
        completion: RevisionNavigationCompletion::RecordJump { origin },
    });

    let _ = app.update(Message::SourceFailed {
        snapshot_id: "stale-commit".to_owned(),
        message: "stale failure".to_owned(),
    });
    assert!(app.revision_navigation.is_some());

    let _ = app.update(Message::SourceFailed {
        snapshot_id: "current-commit".to_owned(),
        message: "source failed".to_owned(),
    });
    assert!(app.revision_navigation.is_none());
}
