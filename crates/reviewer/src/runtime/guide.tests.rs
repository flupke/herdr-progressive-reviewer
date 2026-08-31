use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use herdr_client::protocol::{AgentStatus, PaneId, TabId, WorkspaceId};
use review_guide::{GuideItem, GuideItemStatus, GuideTarget};
use review_repository::repository::RepoType;
use review_test_support::{
    ReviewRepositoryFixture, complete_repository_snapshot, repository_fixture,
};

use super::*;

struct GuideCoordinatorFixture {
    repository_files: Box<dyn ReviewRepositoryFixture>,
    _state_directory: tempfile::TempDir,
    repository: Repository,
    tracker: ReviewTracker,
    guide_store: ReviewStore,
    client: HerdrClient,
    target: AgentTarget,
    snapshot: Snapshot,
    commands: Sender<WorkerCommand>,
    command_receiver: Receiver<WorkerCommand>,
}

impl GuideCoordinatorFixture {
    fn new(repository_type: RepoType) -> Self {
        let repository_files = repository_fixture(repository_type);
        repository_files.write("changed.rs", b"fn before() {}\n");
        repository_files.new_change("base");
        repository_files.write("changed.rs", b"fn after() {}\n");
        repository_files.write("added.rs", b"fn added() {}\n");
        let state_directory = tempfile::tempdir().unwrap();
        let repository = Repository::discover(repository_files.root())
            .unwrap()
            .with_state_root(state_directory.path());
        let snapshot = complete_repository_snapshot(&repository);
        let tracker = ReviewTracker::new(
            repository.clone(),
            ReviewStore::open(state_directory.path(), repository.root()).unwrap(),
        );
        let guide_store = ReviewStore::open(state_directory.path(), repository.root()).unwrap();
        let client = HerdrClient::new(
            state_directory.path().join("missing.sock"),
            "reviewer-test".to_owned(),
            state_directory.path().to_owned(),
        );
        let target = AgentTarget::new(
            WorkspaceId("workspace".to_owned()),
            Some(PaneId("workspace:pane".to_owned())),
        );
        let (commands, command_receiver) = mpsc::channel();
        Self {
            repository_files,
            _state_directory: state_directory,
            repository,
            tracker,
            guide_store,
            client,
            target,
            snapshot,
            commands,
            command_receiver,
        }
    }

    fn context(&mut self) -> GuideOperationContext<'_> {
        GuideOperationContext::new(
            &self.repository,
            &self.tracker,
            &self.guide_store,
            &self.client,
            &mut self.target,
            Some(&self.snapshot),
            &self.commands,
        )
    }

    fn checkpoint(&self) -> ReviewCheckpoint {
        ReviewCheckpoint::new(
            self.snapshot.identity.review_unit().clone(),
            self.snapshot.identity.snapshot_id(),
        )
    }

    fn changed_file(&self, path: &str) -> &ChangedFile {
        self.snapshot
            .files
            .iter()
            .find(|file| file.review_path().display() == path)
            .unwrap()
    }
}

fn agent() -> Agent {
    Agent {
        pane_id: PaneId("workspace:pane".to_owned()),
        tab_id: TabId("tab".to_owned()),
        workspace_id: WorkspaceId("workspace".to_owned()),
        name: None,
        display_agent: None,
        agent: Some("codex".to_owned()),
        agent_status: AgentStatus::Working,
        agent_session: None,
        cwd: None,
    }
}

fn guide_item(path: &str, text: &str) -> GuideItem {
    GuideItem {
        target: GuideTarget::Hunks {
            path: path.to_owned(),
            first_hunk: 1,
            last_hunk: 1,
        },
        text: text.to_owned(),
        status: GuideItemStatus::Matched,
    }
}

fn write_mailbox_response(mailbox: &std::path::Path, text: &str) {
    std::fs::write(
        mailbox.join("response.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "items": [{
                "target": {
                    "kind": "hunks",
                    "path": "changed.rs",
                    "first_hunk": 1,
                    "last_hunk": 1
                },
                "text": text
            }]
        }))
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn display_agent_name_uses_name_display_name_then_pane_id() {
    let mut agent = agent();
    assert_eq!(display_agent_name(&agent), "workspace:pane");
    agent.display_agent = Some("Codex".to_owned());
    assert_eq!(display_agent_name(&agent), "Codex");
    agent.name = Some("Implementer".to_owned());
    assert_eq!(display_agent_name(&agent), "Implementer");
}

#[test_case::test_case(RepoType::Git; "git")]
#[test_case::test_case(RepoType::Jj; "jj")]
fn repository_snapshot_selects_the_scope_and_records_hunk_ranges(repository_type: RepoType) {
    let mut fixture = GuideCoordinatorFixture::new(repository_type);
    let checkpoint = fixture.checkpoint();
    let snapshot = GuideRequestCoordinator::freeze_repository_snapshot(
        &fixture.context(),
        GuideScope::File {
            path: "added.rs".to_owned(),
        },
        &checkpoint,
    )
    .unwrap();

    assert_eq!(snapshot.files.len(), 1);
    assert_eq!(snapshot.files[0].path, "added.rs");
    assert_eq!(snapshot.files[0].hunks[0].old, None);
    assert_eq!(snapshot.files[0].hunks[0].new, Some(0..1));
    assert!(snapshot.frozen_diff.contains("=== HUNK 1 ==="));
    assert!(!snapshot.frozen_diff.contains("changed.rs"));
}

#[test]
fn all_scope_excludes_reviewed_files() {
    let mut fixture = GuideCoordinatorFixture::new(RepoType::Git);
    fixture
        .tracker
        .mark(&fixture.snapshot, fixture.changed_file("changed.rs"))
        .unwrap();
    let checkpoint = fixture.checkpoint();
    let snapshot = GuideRequestCoordinator::freeze_repository_snapshot(
        &fixture.context(),
        GuideScope::All,
        &checkpoint,
    )
    .unwrap();

    assert_eq!(
        snapshot
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        ["added.rs"]
    );
}

#[test]
fn repository_snapshot_preserves_previous_items_for_the_same_checkpoint() {
    let mut fixture = GuideCoordinatorFixture::new(RepoType::Git);
    let checkpoint = fixture.checkpoint();
    let stored = review_guide::GuideSnapshot {
        schema_version: 1,
        review_checkpoint: checkpoint.clone(),
        scope: GuideScope::All,
        items: vec![guide_item("changed.rs", "Previous explanation")],
        anchored_items: Vec::new(),
    };
    fixture.guide_store.save_guide(&stored).unwrap();

    let snapshot = GuideRequestCoordinator::freeze_repository_snapshot(
        &fixture.context(),
        GuideScope::All,
        &checkpoint,
    )
    .unwrap();
    assert_eq!(snapshot.previous_items, stored.items);
}

#[test]
fn frozen_deleted_hunk_has_no_new_line_range() {
    let mut fixture = GuideCoordinatorFixture::new(RepoType::Git);
    fixture
        .repository_files
        .write("removed.rs", b"fn removed() {}\n");
    fixture.repository_files.new_change("add removal target");
    fixture.repository_files.remove("removed.rs");
    fixture.snapshot = complete_repository_snapshot(&fixture.repository);
    let snapshot = fixture.snapshot.clone();
    let file = fixture.changed_file("removed.rs").clone();

    let frozen = GuideRequestCoordinator::frozen_file(&fixture.context(), &snapshot, &file)
        .unwrap()
        .0;

    assert_eq!(frozen.hunks[0].old, Some(0..1));
    assert_eq!(frozen.hunks[0].new, None);
}

#[test]
fn completed_mailbox_response_is_imported_once() {
    let mut fixture = GuideCoordinatorFixture::new(RepoType::Git);
    let checkpoint = fixture.checkpoint();
    let repository_snapshot = GuideRequestCoordinator::freeze_repository_snapshot(
        &fixture.context(),
        GuideScope::All,
        &checkpoint,
    )
    .unwrap();
    let mailbox = fixture
        .guide_store
        .guide_mailbox_directory(&checkpoint.review_unit)
        .unwrap();
    GuideRunner::<HerdrClient>::prepare(repository_snapshot, mailbox.clone()).unwrap();
    write_mailbox_response(&mailbox, "Explanation");
    let (messages, received) = super::super::application_message_channel();
    let mut coordinator = GuideRequestCoordinator::default();

    coordinator.import_completed_guide(&fixture.context(), &messages, &checkpoint.review_unit);
    coordinator.import_completed_guide(&fixture.context(), &messages, &checkpoint.review_unit);

    assert_eq!(
        fixture
            .guide_store
            .load_guide(&checkpoint.review_unit)
            .unwrap()
            .unwrap()
            .items[0]
            .text,
        "Explanation"
    );
    assert_eq!(
        received
            .try_iter()
            .filter(|event| event.downcast_ref::<ReviewGuideChanged>().is_some())
            .count(),
        1
    );
}

#[test]
fn identical_responses_from_different_review_units_are_each_imported() {
    let mut fixture = GuideCoordinatorFixture::new(RepoType::Git);
    let current_checkpoint = fixture.checkpoint();
    let other_checkpoint =
        ReviewCheckpoint::new("other-review-unit", current_checkpoint.checkpoint.clone());
    let mut repository_snapshot = GuideRequestCoordinator::freeze_repository_snapshot(
        &fixture.context(),
        GuideScope::All,
        &current_checkpoint,
    )
    .unwrap();
    repository_snapshot.review_checkpoint = other_checkpoint.clone();
    let current_mailbox = fixture
        .guide_store
        .guide_mailbox_directory(&current_checkpoint.review_unit)
        .unwrap();
    let other_mailbox = fixture
        .guide_store
        .guide_mailbox_directory(&other_checkpoint.review_unit)
        .unwrap();
    let current_repository_snapshot = GuideRequestCoordinator::freeze_repository_snapshot(
        &fixture.context(),
        GuideScope::All,
        &current_checkpoint,
    )
    .unwrap();
    GuideRunner::<HerdrClient>::prepare(current_repository_snapshot, current_mailbox.clone())
        .unwrap();
    GuideRunner::<HerdrClient>::prepare(repository_snapshot, other_mailbox.clone()).unwrap();
    write_mailbox_response(&current_mailbox, "Same explanation");
    write_mailbox_response(&other_mailbox, "Same explanation");
    let (messages, _received) = super::super::application_message_channel();
    let mut coordinator = GuideRequestCoordinator::default();

    coordinator.import_completed_guide(
        &fixture.context(),
        &messages,
        &current_checkpoint.review_unit,
    );
    coordinator.import_completed_guide(
        &fixture.context(),
        &messages,
        &other_checkpoint.review_unit,
    );

    assert!(
        fixture
            .guide_store
            .load_guide(&current_checkpoint.review_unit)
            .unwrap()
            .is_some()
    );
    assert!(
        fixture
            .guide_store
            .load_guide(&other_checkpoint.review_unit)
            .unwrap()
            .is_some()
    );
}

#[test]
fn response_that_lands_after_the_reviewer_opens_is_imported() {
    let mut fixture = GuideCoordinatorFixture::new(RepoType::Git);
    let checkpoint = fixture.checkpoint();
    let repository_snapshot = GuideRequestCoordinator::freeze_repository_snapshot(
        &fixture.context(),
        GuideScope::All,
        &checkpoint,
    )
    .unwrap();
    let mailbox = fixture
        .guide_store
        .guide_mailbox_directory(&checkpoint.review_unit)
        .unwrap();
    GuideRunner::<HerdrClient>::prepare(repository_snapshot, mailbox.clone()).unwrap();
    let (messages, _received) = super::super::application_message_channel();
    let mut coordinator = GuideRequestCoordinator::default();

    coordinator.import_completed_guide(&fixture.context(), &messages, &checkpoint.review_unit);
    write_mailbox_response(&mailbox, "Late explanation");
    let command = fixture
        .command_receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap();
    let WorkerCommand::ImportReviewGuide {
        review_unit,
        wait_token,
    } = command
    else {
        panic!("expected a completed mailbox import command");
    };

    coordinator.response_ready(&fixture.context(), &messages, &review_unit, wait_token);

    assert_eq!(
        fixture
            .guide_store
            .load_guide(&checkpoint.review_unit)
            .unwrap()
            .unwrap()
            .items[0]
            .text,
        "Late explanation"
    );
}

#[test]
fn stale_completion_imports_the_last_response_that_landed() {
    let mut fixture = GuideCoordinatorFixture::new(RepoType::Git);
    let checkpoint = fixture.checkpoint();
    let repository_snapshot = GuideRequestCoordinator::freeze_repository_snapshot(
        &fixture.context(),
        GuideScope::All,
        &checkpoint,
    )
    .unwrap();
    let mailbox_directory = fixture
        .guide_store
        .guide_mailbox_directory(&checkpoint.review_unit)
        .unwrap();
    GuideRunner::<HerdrClient>::prepare(repository_snapshot, mailbox_directory.clone()).unwrap();
    write_mailbox_response(&mailbox_directory, "Older response");
    let older_result = GuideMailbox::open(mailbox_directory.clone())
        .unwrap()
        .load_completed_guide()
        .unwrap();
    write_mailbox_response(&mailbox_directory, "Newest response");
    let (messages, _received) = super::super::application_message_channel();
    let mut coordinator = GuideRequestCoordinator::default();

    coordinator.finish_review_guide(
        &fixture.context(),
        &messages,
        FinishedGuide {
            review_checkpoint: checkpoint.clone(),
            agent_name: "Codex".to_owned(),
            result: Ok(older_result),
        },
    );

    assert_eq!(
        fixture
            .guide_store
            .load_guide(&checkpoint.review_unit)
            .unwrap()
            .unwrap()
            .items[0]
            .text,
        "Newest response"
    );
}

#[test]
fn exact_checkpoint_guide_is_shown() {
    let mut fixture = GuideCoordinatorFixture::new(RepoType::Git);
    let checkpoint = fixture.checkpoint();
    let guide = review_guide::GuideSnapshot {
        schema_version: 1,
        review_checkpoint: checkpoint.clone(),
        scope: GuideScope::All,
        items: vec![guide_item("changed.rs", "Explanation")],
        anchored_items: Vec::new(),
    };
    let (messages, received) = super::super::application_message_channel();

    GuideRequestCoordinator::show_guide_for_current_checkpoint(
        &fixture.context(),
        &messages,
        &guide,
    );

    let envelope = received.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(matches!(
        envelope.downcast_ref::<ReviewGuideChanged>(),
        Some(event) if event.review_checkpoint == checkpoint && event.items == guide.items
    ));
}
