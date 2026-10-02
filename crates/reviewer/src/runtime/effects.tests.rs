use std::path::PathBuf;

use review_repository::repository::{ChangeId, RepoType};
use review_source::ReviewCheckpoint;
use review_test_support::repository_fixture;
use review_threads::ThreadCommand;
use review_ui::SourceLoadMode;
use ui_events::{
    DiffContentLoaded, ExploreRestored, RepositoryFilesChanged, RepositoryMetadataChanged,
    RepositoryRefreshFinished, ReviewThreadsLoaded, RevisionEditFailed, SourceContentLoaded,
};

use super::fixture::EffectsFixture;
use super::*;

fn load_diff(checkpoint: &ReviewCheckpoint, path: &str) -> Action {
    Action::Document(DocumentAction::Load(DocumentLoad::Diff {
        review_checkpoint: checkpoint.clone(),
        path: path.to_owned(),
    }))
}

fn event_names(events: &[EventEnvelope]) -> Vec<&'static str> {
    events
        .iter()
        .filter_map(|event| {
            if event.downcast_ref::<RepositoryMetadataChanged>().is_some() {
                Some("metadata")
            } else if event.downcast_ref::<RepositoryFilesChanged>().is_some() {
                Some("files")
            } else if event.downcast_ref::<ExploreRestored>().is_some() {
                Some("explore restored")
            } else if event.downcast_ref::<RepositoryRefreshFinished>().is_some() {
                Some("refresh finished")
            } else {
                None
            }
        })
        .collect()
}

#[test_case::test_case(RepoType::Git; "git")]
#[test_case::test_case(RepoType::Jj; "jj")]
fn a_refresh_publishes_files_the_documents_can_load_then_restores_explore(kind: RepoType) {
    let files = repository_fixture(kind);
    files.write("src/lib.rs", b"pub fn refreshed() {}\n");
    let mut fixture = EffectsFixture::start(files, |_| {});

    let events = fixture.refresh();

    assert_eq!(
        event_names(&events),
        ["metadata", "files", "explore restored", "refresh finished"]
    );
    let files = events
        .iter()
        .find_map(|event| event.downcast_ref::<RepositoryFilesChanged>())
        .unwrap();
    // The files event can only be acted on once the documents hold its snapshot.
    fixture.perform([load_diff(&files.review_checkpoint, "src/lib.rs")]);
    let loaded = fixture.wait_for::<DiffContentLoaded>();
    assert_eq!(loaded.review_checkpoint, files.review_checkpoint);
    assert_eq!(
        loaded.new_content.as_deref(),
        Some(b"pub fn refreshed() {}\n".as_slice())
    );

    fixture.files.write("src/lib.rs", b"pub fn changed() {}\n");
    let events = fixture.refresh();
    // The same review keeps its restored Explore round.
    assert_eq!(
        event_names(&events),
        ["metadata", "files", "refresh finished"]
    );
}

#[test_case::test_case("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz", "immutable or unavailable"; "immutable root")]
#[test_case::test_case("unknownchange", "read jj repository failed"; "unknown change")]
fn a_failed_revision_edit_reports_why_and_keeps_the_comparison(change_id: &str, reason: &str) {
    let files = repository_fixture(RepoType::Jj);
    files.write("src/lib.rs", b"pub fn current() {}\n");
    let mut fixture = EffectsFixture::start(files, |_| {});
    let checkpoint = fixture.refreshed_checkpoint();

    fixture.perform([Action::Repository(RepositoryAction::EditRevision {
        change_id: ChangeId::from(change_id.to_owned()),
    })]);

    let events = fixture.events_until::<RevisionEditFailed>();
    let failure = events.last().unwrap().downcast_ref::<RevisionEditFailed>();
    let message = failure.unwrap().message.clone().unwrap();
    assert!(message.contains(reason), "{message}");
    assert!(event_names(&events).is_empty(), "no refresh on failure");
    assert_eq!(fixture.refreshed_checkpoint(), checkpoint);
}

#[test]
fn documents_and_threads_do_not_wait_for_repository_work() {
    let files = repository_fixture(RepoType::Git);
    files.write("changed.rs", b"fn original() {}\n");
    files.new_change("original");
    files.write("changed.rs", b"fn updated() {}\n");
    let mut fixture = EffectsFixture::start(files, |_| {});
    let checkpoint = fixture.refreshed_checkpoint();
    let hold = fixture.effects.hold_repository_work();
    fixture.effects.refresh().unwrap();

    fixture.perform([
        Action::Document(DocumentAction::Load(DocumentLoad::Diffs {
            review_checkpoint: checkpoint.clone(),
            paths: vec!["changed.rs".to_owned()],
        })),
        Action::Document(DocumentAction::Load(DocumentLoad::Source {
            snapshot_id: checkpoint.checkpoint.clone(),
            location: review_lsp::SourceLocation {
                path: PathBuf::from("changed.rs"),
                line: 0,
                byte_column: 0,
                end_line: 0,
                end_byte_column: 0,
            },
            mode: SourceLoadMode::External,
        })),
        Action::Thread(ThreadCommand::Load(checkpoint.review_unit.clone())),
    ]);

    let (mut diff, mut source, mut threads) = (None, None, false);
    while diff.is_none() || source.is_none() || !threads {
        let event = fixture.next_event();
        assert!(
            event_names(std::slice::from_ref(&event)).is_empty(),
            "the held refresh ran first"
        );
        diff = diff.or_else(|| event.downcast_ref::<DiffContentLoaded>().cloned());
        source = source.or_else(|| event.downcast_ref::<SourceContentLoaded>().cloned());
        threads |= event.downcast_ref::<ReviewThreadsLoaded>().is_some();
    }
    assert_eq!(
        diff.unwrap().new_content.as_deref(),
        Some(b"fn updated() {}\n".as_slice())
    );
    let source = source.unwrap();
    assert_eq!(source.content, b"fn updated() {}\n");
    assert_eq!(
        source.location.path,
        fixture.repository.root().join("changed.rs"),
        "relative paths resolve against the repository root"
    );
    drop(hold);
    fixture.wait_for::<RepositoryRefreshFinished>();
}

#[test]
fn settings_save_and_terminal_actions_run_in_order_until_quit() {
    let fixture = EffectsFixture::new(RepoType::Git);
    let mut opened = Vec::new();

    let flow = fixture
        .effects
        .perform_all(
            vec![
                Action::Settings(SettingsAction::SaveFilePaneWidth(42)),
                Action::Terminal(TerminalAction::OpenInEditor {
                    path: PathBuf::from("src/lib.rs"),
                    line: Some(7),
                }),
                Action::Terminal(TerminalAction::Quit),
                Action::Settings(SettingsAction::SaveFilePaneWidth(7)),
            ],
            &mut |action| match action {
                TerminalAction::OpenInEditor { path, line } => {
                    opened.push((path, line));
                    Ok(ControlFlow::Continue(()))
                }
                TerminalAction::Quit => Ok(ControlFlow::Break(())),
            },
        )
        .unwrap();

    assert_eq!(flow, ControlFlow::Break(()));
    assert_eq!(fixture.store.file_pane_width().unwrap(), Some(42));
    assert_eq!(opened, [(PathBuf::from("src/lib.rs"), Some(7))]);
}

#[test]
fn source_loads_prefer_frozen_content_when_a_deleted_path_is_recreated() {
    let files = repository_fixture(RepoType::Git);
    let deleted_content = b"fn deleted_from_worktree() {}\n";
    files.write("deleted.rs", deleted_content);
    files.new_change("add the file that the next change deletes");
    files.remove("deleted.rs");
    let mut fixture = EffectsFixture::start(files, |_| {});
    let checkpoint = fixture.refreshed_checkpoint();
    let path = fixture.repository.root().join("deleted.rs");
    std::fs::write(&path, "fn recreated_after_snapshot() {}\n").unwrap();
    let load = |mode| {
        Action::Document(DocumentAction::Load(DocumentLoad::Source {
            snapshot_id: checkpoint.checkpoint.clone(),
            location: review_lsp::SourceLocation {
                path: path.clone(),
                line: 0,
                byte_column: 0,
                end_line: 0,
                end_byte_column: 0,
            },
            mode,
        }))
    };

    fixture.perform([load(SourceLoadMode::External)]);
    let loaded = fixture.wait_for::<SourceContentLoaded>();
    assert_eq!(loaded.snapshot_id, checkpoint.checkpoint);
    assert_eq!(loaded.location.path, path);
    assert_eq!(loaded.mode, SourceLoadMode::External);
    assert_eq!(loaded.content, deleted_content);

    fixture.perform([load(SourceLoadMode::ThreadPeek)]);
    assert_eq!(
        fixture.wait_for::<SourceContentLoaded>().content,
        b"fn recreated_after_snapshot() {}\n"
    );

    std::fs::remove_file(&path).unwrap();
    fixture.perform([load(SourceLoadMode::ThreadPeek)]);
    fixture.wait_for::<ui_events::SourceContentLoadFailed>();
}
