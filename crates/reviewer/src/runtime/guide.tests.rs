use std::fs::OpenOptions;
use std::os::unix::fs::OpenOptionsExt;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use herdr_client::protocol::{AgentSession, PaneId, TabId, WorkspaceId};
use review_guide::{GuideItem, GuideItemStatus, GuideTarget};
use review_repository::repository::RepoType;
use review_test_support::{
    ReviewRepositoryFixture, complete_repository_snapshot, repository_fixture,
};

use super::*;

fn agent() -> Agent {
    Agent {
        pane_id: PaneId("workspace:pane".to_owned()),
        tab_id: TabId("tab".to_owned()),
        workspace_id: WorkspaceId("workspace".to_owned()),
        name: None,
        display_agent: None,
        agent: Some("codex".to_owned()),
        agent_status: AgentStatus::Working,
        agent_session: Some(AgentSession {
            source: "herdr:codex".to_owned(),
            agent: "codex".to_owned(),
            kind: "id".to_owned(),
            value: "session".to_owned(),
        }),
        cwd: None,
    }
}

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
            self.snapshot.identity.review_id(),
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

fn request_record(
    checkpoint: ReviewCheckpoint,
    request_id: &str,
    state: GuideRequestState,
) -> GuideRequestRecord {
    GuideRequestRecord {
        schema_version: 1,
        review_checkpoint: checkpoint,
        request_id: GuideRequestId::new(request_id),
        scope: GuideScope::All,
        agent: agent(),
        state,
        created_at: String::new(),
        updated_at: String::new(),
        transport_directory: None,
        error: None,
    }
}

fn active_guide(record: GuideRequestRecord) -> ActiveGuide {
    ActiveGuide {
        request_id: record.request_id.clone(),
        agent: record.agent.clone(),
        cancellation: Arc::new(GuideCancellation::default()),
        record,
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

fn mark_transport_submitted(prepared: &PreparedGuide) {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(prepared.transport_directory().join("submitted"))
        .unwrap();
}

fn detection(released: bool, reported_agent: &str) -> HerdrEvent {
    HerdrEvent::AgentDetected {
        pane_id: PaneId("workspace:pane".to_owned()),
        workspace_id: WorkspaceId("workspace".to_owned()),
        agent: Some(reported_agent.to_owned()),
        released,
        final_status: None,
    }
}

#[test]
fn same_agent_detection_does_not_cancel_a_guide() {
    assert_eq!(
        classify_guide_agent_event(&agent(), &detection(false, "codex")),
        GuideAgentEvent::Ignore
    );
}

#[test]
fn agent_release_does_not_cancel_a_guide() {
    assert_eq!(
        classify_guide_agent_event(&agent(), &detection(true, "codex")),
        GuideAgentEvent::Ignore
    );
}

#[test]
fn agent_detection_is_not_used_as_replacement_evidence() {
    assert_eq!(
        classify_guide_agent_event(&agent(), &detection(false, "claude")),
        GuideAgentEvent::Ignore
    );
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

#[test]
fn only_blocked_status_for_the_exact_agent_blocks_a_guide() {
    let active = agent();
    let status_event = |pane_id: &str, workspace_id: &str, status| HerdrEvent::AgentStatusChanged {
        pane_id: PaneId(pane_id.to_owned()),
        workspace_id: WorkspaceId(workspace_id.to_owned()),
        agent: Some("codex".to_owned()),
        status,
    };
    assert_eq!(
        classify_guide_agent_event(
            &active,
            &status_event("workspace:pane", "workspace", AgentStatus::Blocked)
        ),
        GuideAgentEvent::Block
    );
    assert_eq!(
        classify_guide_agent_event(
            &active,
            &status_event("other", "workspace", AgentStatus::Blocked)
        ),
        GuideAgentEvent::Ignore
    );
    assert_eq!(
        classify_guide_agent_event(
            &active,
            &status_event("workspace:pane", "other", AgentStatus::Blocked)
        ),
        GuideAgentEvent::Ignore
    );
    assert_eq!(
        classify_guide_agent_event(
            &active,
            &status_event("workspace:pane", "workspace", AgentStatus::Working)
        ),
        GuideAgentEvent::Ignore
    );
}

#[test_case::test_case(RepoType::Git; "git")]
#[test_case::test_case(RepoType::Jj; "jj")]
fn request_freezing_selects_the_scope_and_records_exact_hunk_ranges(repository_type: RepoType) {
    let mut fixture = GuideCoordinatorFixture::new(repository_type);
    let checkpoint = fixture.checkpoint();
    let request = GuideRequestCoordinator::freeze_guide_request(
        &fixture.context(),
        GuideScope::File {
            path: "added.rs".to_owned(),
        },
        &checkpoint,
    )
    .unwrap();

    assert_eq!(request.files.len(), 1);
    assert_eq!(request.files[0].path, "added.rs");
    assert_eq!(request.files[0].hunk_count, 1);
    assert_eq!(request.files[0].hunks[0].old, None);
    assert_eq!(request.files[0].hunks[0].new, Some(0..1));
    assert!(request.frozen_diff.contains("=== HUNK 1 ==="));
    assert!(!request.frozen_diff.contains("changed.rs"));

    let snapshot = fixture.snapshot.clone();
    let file = fixture.changed_file("changed.rs").clone();
    let changed = GuideRequestCoordinator::frozen_file(&fixture.context(), &snapshot, &file)
        .unwrap()
        .0;
    assert_eq!(changed.hunks[0].old, Some(0..1));
    assert_eq!(changed.hunks[0].new, Some(0..1));
}

#[test]
fn all_scope_excludes_reviewed_files() {
    let mut fixture = GuideCoordinatorFixture::new(RepoType::Git);
    fixture
        .tracker
        .mark(&fixture.snapshot, fixture.changed_file("changed.rs"))
        .unwrap();
    let checkpoint = fixture.checkpoint();
    let request = GuideRequestCoordinator::freeze_guide_request(
        &fixture.context(),
        GuideScope::All,
        &checkpoint,
    )
    .unwrap();

    assert_eq!(
        request
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        ["added.rs"]
    );
}

#[test]
fn freezing_preserves_previous_items_only_for_the_same_checkpoint() {
    let mut fixture = GuideCoordinatorFixture::new(RepoType::Git);
    let checkpoint = fixture.checkpoint();
    let stored = review_guide::GuideSnapshot {
        schema_version: 1,
        review_checkpoint: checkpoint.clone(),
        request_id: GuideRequestId::new("previous"),
        scope: GuideScope::All,
        items: vec![guide_item("changed.rs", "Previous explanation")],
        anchored_items: Vec::new(),
    };
    fixture.guide_store.save_guide(&stored).unwrap();

    let same = GuideRequestCoordinator::freeze_guide_request(
        &fixture.context(),
        GuideScope::All,
        &checkpoint,
    )
    .unwrap();
    assert_eq!(same.previous_items, stored.items);

    let later_checkpoint = ReviewCheckpoint::new(&checkpoint.review_unit, "later");
    let later = GuideRequestCoordinator::freeze_guide_request(
        &fixture.context(),
        GuideScope::All,
        &later_checkpoint,
    )
    .unwrap();
    assert!(later.previous_items.is_empty());
}

#[test]
fn freezing_marks_a_file_without_text_hunks() {
    let mut fixture = GuideCoordinatorFixture::new(RepoType::Git);
    fixture.repository_files.write("binary.bin", b"\0binary");
    fixture.snapshot = complete_repository_snapshot(&fixture.repository);
    let checkpoint = fixture.checkpoint();

    let request = GuideRequestCoordinator::freeze_guide_request(
        &fixture.context(),
        GuideScope::File {
            path: "binary.bin".to_owned(),
        },
        &checkpoint,
    )
    .unwrap();

    assert_eq!(request.files[0].hunk_count, 0);
    assert!(request.frozen_diff.contains("=== NO TEXT HUNKS ==="));
    assert!(!request.frozen_diff.contains("=== HUNK 1 ==="));
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
fn previous_items_from_reviewed_files_are_not_carried_forward() {
    let mut fixture = GuideCoordinatorFixture::new(RepoType::Git);
    let checkpoint = fixture.checkpoint();
    let changed_file = fixture.changed_file("changed.rs").clone();
    let snapshot = fixture.snapshot.clone();
    let frozen_file =
        GuideRequestCoordinator::frozen_file(&fixture.context(), &snapshot, &changed_file)
            .unwrap()
            .0;
    let items = vec![guide_item("changed.rs", "Previous explanation")];
    let anchored_items =
        review_guide::anchor_items(&items, std::slice::from_ref(&frozen_file), "old-checkpoint");
    fixture
        .guide_store
        .save_guide(&review_guide::GuideSnapshot {
            schema_version: 1,
            review_checkpoint: ReviewCheckpoint::new(&checkpoint.review_unit, "old-checkpoint"),
            request_id: GuideRequestId::new("previous"),
            scope: GuideScope::All,
            items,
            anchored_items: anchored_items.clone(),
        })
        .unwrap();
    fixture
        .tracker
        .mark(&fixture.snapshot, &changed_file)
        .unwrap();

    let request = GuideRequestCoordinator::freeze_guide_request(
        &fixture.context(),
        GuideScope::All,
        &checkpoint,
    )
    .unwrap();

    assert!(request.previous_items.is_empty());
    assert_eq!(request.previous_anchored_items, anchored_items);
}

#[test]
fn submitted_and_stopped_requests_are_persisted_for_only_the_active_request() {
    let mut fixture = GuideCoordinatorFixture::new(RepoType::Git);
    let checkpoint = fixture.checkpoint();
    let record = request_record(checkpoint.clone(), "active", GuideRequestState::Preparing);
    let mut coordinator = GuideRequestCoordinator {
        active: Some(active_guide(record)),
        reconciled_review_unit: None,
    };

    coordinator.mark_submitted(&fixture.context(), &GuideRequestId::new("other"));
    assert!(
        fixture
            .guide_store
            .load_guide_requests(&checkpoint.review_unit)
            .unwrap()
            .is_empty()
    );
    coordinator.mark_submitted(&fixture.context(), &GuideRequestId::new("active"));
    let submitted = fixture
        .guide_store
        .load_guide_requests(&checkpoint.review_unit)
        .unwrap();
    assert_eq!(submitted[0].state, GuideRequestState::Submitted);

    coordinator.active.as_mut().unwrap().record.error = Some("old error".to_owned());
    coordinator.stop(&fixture.context());
    let stopped = fixture
        .guide_store
        .load_guide_requests(&checkpoint.review_unit)
        .unwrap();
    assert_eq!(stopped[0].state, GuideRequestState::Incomplete);
    assert_eq!(stopped[0].error, None);
}

#[test]
fn completion_clears_transport_and_limits_the_stored_error() {
    let mut fixture = GuideCoordinatorFixture::new(RepoType::Git);
    let checkpoint = fixture.checkpoint();
    let mut record = request_record(checkpoint.clone(), "request", GuideRequestState::Submitted);
    record.transport_directory = Some(fixture.repository_files.root().to_owned());
    let long_error = "x".repeat(600);

    GuideRequestCoordinator::complete_request_record(
        &fixture.context(),
        &mut record,
        GuideRequestState::Failed,
        Some(long_error),
    )
    .unwrap();

    let stored = fixture
        .guide_store
        .load_guide_requests(&checkpoint.review_unit)
        .unwrap();
    assert_eq!(stored[0].state, GuideRequestState::Failed);
    assert_eq!(stored[0].transport_directory, None);
    assert_eq!(stored[0].error.as_ref().unwrap().chars().count(), 512);
}

#[test]
fn exact_checkpoint_guide_is_shown_and_another_review_unit_is_ignored() {
    let mut fixture = GuideCoordinatorFixture::new(RepoType::Git);
    let checkpoint = fixture.checkpoint();
    let guide = review_guide::GuideSnapshot {
        schema_version: 1,
        review_checkpoint: checkpoint.clone(),
        request_id: GuideRequestId::new("request"),
        scope: GuideScope::All,
        items: vec![guide_item("changed.rs", "Explanation")],
        anchored_items: Vec::new(),
    };
    let (messages, received) = mpsc::channel();

    GuideRequestCoordinator::show_guide_for_current_checkpoint(
        &fixture.context(),
        &messages,
        &guide,
    );
    assert!(matches!(
        received.recv_timeout(Duration::from_secs(1)).unwrap(),
        Message::ReviewGuideLoaded { review_checkpoint, items }
            if review_checkpoint == checkpoint && items == guide.items
    ));

    let mut other = guide;
    other.review_checkpoint.review_unit = "other".to_owned();
    GuideRequestCoordinator::show_guide_for_current_checkpoint(
        &fixture.context(),
        &messages,
        &other,
    );
    assert!(received.try_recv().is_err());
}

#[test]
fn showing_an_old_guide_does_not_map_items_from_reviewed_files() {
    let mut fixture = GuideCoordinatorFixture::new(RepoType::Git);
    let checkpoint = fixture.checkpoint();
    let changed_file = fixture.changed_file("changed.rs").clone();
    let snapshot = fixture.snapshot.clone();
    let frozen_file =
        GuideRequestCoordinator::frozen_file(&fixture.context(), &snapshot, &changed_file)
            .unwrap()
            .0;
    let items = vec![guide_item("changed.rs", "Old explanation")];
    let guide = review_guide::GuideSnapshot {
        schema_version: 1,
        review_checkpoint: ReviewCheckpoint::new(&checkpoint.review_unit, "old-checkpoint"),
        request_id: GuideRequestId::new("request"),
        scope: GuideScope::All,
        anchored_items: review_guide::anchor_items(&items, &[frozen_file], "old-checkpoint"),
        items,
    };
    fixture
        .tracker
        .mark(&fixture.snapshot, &changed_file)
        .unwrap();
    let (messages, received) = mpsc::channel();

    GuideRequestCoordinator::show_guide_for_current_checkpoint(
        &fixture.context(),
        &messages,
        &guide,
    );

    assert!(matches!(
        received.recv_timeout(Duration::from_secs(1)).unwrap(),
        Message::ReviewGuideLoaded { review_checkpoint, items }
            if review_checkpoint == checkpoint && items.is_empty()
    ));
}

#[test]
fn old_guide_items_are_mapped_for_unreviewed_files() {
    let mut fixture = GuideCoordinatorFixture::new(RepoType::Git);
    let checkpoint = fixture.checkpoint();
    let changed_file = fixture.changed_file("changed.rs").clone();
    let snapshot = fixture.snapshot.clone();
    let frozen_file =
        GuideRequestCoordinator::frozen_file(&fixture.context(), &snapshot, &changed_file)
            .unwrap()
            .0;
    let items = vec![guide_item("changed.rs", "Old explanation")];
    let guide = review_guide::GuideSnapshot {
        schema_version: 1,
        review_checkpoint: ReviewCheckpoint::new(&checkpoint.review_unit, "old-checkpoint"),
        request_id: GuideRequestId::new("request"),
        scope: GuideScope::All,
        anchored_items: review_guide::anchor_items(&items, &[frozen_file], "old-checkpoint"),
        items,
    };
    fixture.guide_store.save_guide(&guide).unwrap();
    let (messages, received) = mpsc::channel();

    GuideRequestCoordinator::show_guide_for_current_checkpoint(
        &fixture.context(),
        &messages,
        &guide,
    );
    assert!(matches!(
        received.recv_timeout(Duration::from_secs(1)).unwrap(),
        Message::ReviewGuideLoaded { items, .. }
            if items.len() == 1 && items[0].text == "Old explanation"
    ));
    let request = GuideRequestCoordinator::freeze_guide_request(
        &fixture.context(),
        GuideScope::All,
        &checkpoint,
    )
    .unwrap();
    assert_eq!(request.previous_items.len(), 1);
    assert_eq!(request.previous_items[0].text, "Old explanation");
}

#[test]
fn status_helpers_report_generation_state() {
    let checkpoint = ReviewCheckpoint::new("review", "checkpoint");
    let (messages, received) = mpsc::channel();
    GuideRequestCoordinator::updating_status(&messages, &checkpoint, Some("working".to_owned()));
    assert!(matches!(
        received.recv_timeout(Duration::from_secs(1)).unwrap(),
        Message::ReviewGuideStatus { review_checkpoint, generating: true, message: Some(message) }
            if review_checkpoint == checkpoint && message == "working"
    ));
}

#[test]
fn reconciliation_resumes_only_the_newest_valid_incomplete_request() {
    let mut fixture = GuideCoordinatorFixture::new(RepoType::Git);
    let checkpoint = fixture.checkpoint();
    let request = GuideRequestCoordinator::freeze_guide_request(
        &fixture.context(),
        GuideScope::All,
        &checkpoint,
    )
    .unwrap();
    let older_prepared = GuideRunner::<HerdrClient>::prepare(request).unwrap();
    mark_transport_submitted(&older_prepared);
    let mut older = request_record(
        checkpoint.clone(),
        older_prepared.request_id().as_str(),
        GuideRequestState::Submitted,
    );
    older.transport_directory = Some(older_prepared.transport_directory().to_owned());
    fixture.guide_store.save_guide_request(&mut older).unwrap();

    let request = GuideRequestCoordinator::freeze_guide_request(
        &fixture.context(),
        GuideScope::All,
        &checkpoint,
    )
    .unwrap();
    let newer_prepared = GuideRunner::<HerdrClient>::prepare(request).unwrap();
    mark_transport_submitted(&newer_prepared);
    let mut newer = request_record(
        checkpoint.clone(),
        newer_prepared.request_id().as_str(),
        GuideRequestState::Incomplete,
    );
    newer.transport_directory = Some(newer_prepared.transport_directory().to_owned());
    fixture.guide_store.save_guide_request(&mut newer).unwrap();

    let (messages, _received) = mpsc::channel();
    let mut coordinator = GuideRequestCoordinator::default();
    coordinator.reconcile_incomplete_guides(
        &mut fixture.context(),
        &messages,
        &checkpoint.review_unit,
    );

    assert_eq!(
        coordinator.active.as_ref().map(|active| &active.request_id),
        Some(&newer.request_id)
    );
    let records = fixture
        .guide_store
        .load_guide_requests(&checkpoint.review_unit)
        .unwrap();
    assert_eq!(
        records
            .iter()
            .find(|record| record.request_id == older.request_id)
            .unwrap()
            .state,
        GuideRequestState::Failed
    );

    coordinator.stop(&fixture.context());
    assert!(matches!(
        fixture
            .command_receiver
            .recv_timeout(Duration::from_secs(2))
            .unwrap(),
        WorkerCommand::GuideFinished(FinishedGuide { request_id, .. })
            if request_id == newer.request_id
    ));
}

#[test]
fn reconciliation_rejects_invalid_transports_and_runs_once_per_review_unit() {
    let mut fixture = GuideCoordinatorFixture::new(RepoType::Git);
    let checkpoint = fixture.checkpoint();
    let mut invalid = request_record(checkpoint.clone(), "invalid", GuideRequestState::Preparing);
    invalid.transport_directory = Some(fixture.repository_files.root().join("missing"));
    fixture
        .guide_store
        .save_guide_request(&mut invalid)
        .unwrap();
    let (messages, _received) = mpsc::channel();
    let mut coordinator = GuideRequestCoordinator::default();

    coordinator.reconcile_incomplete_guides(
        &mut fixture.context(),
        &messages,
        &checkpoint.review_unit,
    );
    let failed = fixture
        .guide_store
        .load_guide_requests(&checkpoint.review_unit)
        .unwrap();
    assert_eq!(failed[0].state, GuideRequestState::Failed);
    assert!(failed[0].error.is_some());

    let mut later = request_record(checkpoint.clone(), "later", GuideRequestState::Incomplete);
    fixture.guide_store.save_guide_request(&mut later).unwrap();
    coordinator.reconcile_incomplete_guides(
        &mut fixture.context(),
        &messages,
        &checkpoint.review_unit,
    );
    let records = fixture
        .guide_store
        .load_guide_requests(&checkpoint.review_unit)
        .unwrap();
    assert_eq!(
        records
            .iter()
            .find(|record| record.request_id == later.request_id)
            .unwrap()
            .state,
        GuideRequestState::Incomplete
    );
}

#[test]
fn reconciliation_does_not_replace_an_active_request() {
    let mut fixture = GuideCoordinatorFixture::new(RepoType::Git);
    let checkpoint = fixture.checkpoint();
    let active_record = request_record(checkpoint.clone(), "active", GuideRequestState::Submitted);
    let request = GuideRequestCoordinator::freeze_guide_request(
        &fixture.context(),
        GuideScope::All,
        &checkpoint,
    )
    .unwrap();
    let prepared = GuideRunner::<HerdrClient>::prepare(request).unwrap();
    mark_transport_submitted(&prepared);
    let mut candidate = request_record(
        checkpoint.clone(),
        prepared.request_id().as_str(),
        GuideRequestState::Incomplete,
    );
    candidate.transport_directory = Some(prepared.transport_directory().to_owned());
    fixture
        .guide_store
        .save_guide_request(&mut candidate)
        .unwrap();
    let mut coordinator = GuideRequestCoordinator {
        active: Some(active_guide(active_record.clone())),
        reconciled_review_unit: None,
    };
    let (messages, _received) = mpsc::channel();

    coordinator.reconcile_incomplete_guides(
        &mut fixture.context(),
        &messages,
        &checkpoint.review_unit,
    );

    assert_eq!(
        coordinator.active.as_ref().map(|active| &active.request_id),
        Some(&active_record.request_id)
    );
    let records = fixture
        .guide_store
        .load_guide_requests(&checkpoint.review_unit)
        .unwrap();
    assert_eq!(records[0].state, GuideRequestState::Failed);
}

#[test]
fn finishing_the_active_request_stores_and_reports_the_guide() {
    let mut fixture = GuideCoordinatorFixture::new(RepoType::Git);
    let checkpoint = fixture.checkpoint();
    let record = request_record(checkpoint.clone(), "request", GuideRequestState::Submitted);
    let guide = review_guide::GuideSnapshot {
        schema_version: 1,
        review_checkpoint: checkpoint.clone(),
        request_id: record.request_id.clone(),
        scope: GuideScope::All,
        items: vec![guide_item("changed.rs", "Explanation")],
        anchored_items: Vec::new(),
    };
    let mut coordinator = GuideRequestCoordinator {
        active: Some(active_guide(record.clone())),
        reconciled_review_unit: None,
    };
    let (messages, received) = mpsc::channel();

    coordinator.finish_review_guide(
        &mut fixture.context(),
        &messages,
        FinishedGuide {
            request_id: GuideRequestId::new("other"),
            review_checkpoint: checkpoint.clone(),
            agent_name: "Codex".to_owned(),
            result: Ok(GuideResult {
                guide: guide.clone(),
                rejected_items: 1,
            }),
        },
    );
    assert!(coordinator.active.is_some());
    coordinator.finish_review_guide(
        &mut fixture.context(),
        &messages,
        FinishedGuide {
            request_id: record.request_id.clone(),
            review_checkpoint: checkpoint.clone(),
            agent_name: "Codex".to_owned(),
            result: Ok(GuideResult {
                guide: guide.clone(),
                rejected_items: 1,
            }),
        },
    );

    assert!(coordinator.active.is_none());
    assert_eq!(
        fixture
            .guide_store
            .load_guide(&checkpoint.review_unit)
            .unwrap(),
        Some(guide)
    );
    let records = fixture
        .guide_store
        .load_guide_requests(&checkpoint.review_unit)
        .unwrap();
    assert_eq!(records[0].state, GuideRequestState::Completed);
    assert_eq!(records[0].transport_directory, None);
    let messages = received.try_iter().collect::<Vec<_>>();
    assert!(messages.iter().any(|message| matches!(
        message,
        Message::ReviewGuideStatus { generating: false, message: Some(text), .. }
            if text == "invalid review guide items ignored: 1"
    )));
}

#[test]
fn exact_blocked_agent_event_interrupts_the_active_request() {
    let mut fixture = GuideCoordinatorFixture::new(RepoType::Git);
    let checkpoint = fixture.checkpoint();
    let request = GuideRequestCoordinator::freeze_guide_request(
        &fixture.context(),
        GuideScope::All,
        &checkpoint,
    )
    .unwrap();
    let prepared = GuideRunner::<HerdrClient>::prepare(request).unwrap();
    let record = request_record(
        checkpoint,
        prepared.request_id().as_str(),
        GuideRequestState::Submitted,
    );
    let cancellation = Arc::new(GuideCancellation::default());
    let mut coordinator = GuideRequestCoordinator {
        active: Some(ActiveGuide {
            request_id: record.request_id.clone(),
            agent: record.agent.clone(),
            cancellation: Arc::clone(&cancellation),
            record,
        }),
        reconciled_review_unit: None,
    };
    coordinator.observe_guide_event(&HerdrEvent::AgentStatusChanged {
        pane_id: PaneId("workspace:pane".to_owned()),
        workspace_id: WorkspaceId("workspace".to_owned()),
        agent: Some("codex".to_owned()),
        status: AgentStatus::Blocked,
    });

    assert!(matches!(
        GuideRunner::new(&fixture.client).finish_prepared(&agent(), prepared, &cancellation),
        Err(review_guide_runner::Error::AgentBlocked)
    ));
}
