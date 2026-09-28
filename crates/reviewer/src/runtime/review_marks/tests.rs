use super::*;
use crate::runtime::{comment_service, explore};
use herdr_client::{
    client::HerdrClient,
    protocol::{AgentTarget, WorkspaceId},
};
use review_repository::repository::{RepoType, Repository};
use review_state::{ReviewStatus, ReviewTracker};
use review_store::{LoadResult, ReviewStore};
use review_test_support::{complete_repository_snapshot, repository_fixture};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

#[test_case::test_case(RepoType::Git; "git")]
#[test_case::test_case(RepoType::Jj; "jj")]
fn bulk_reset_rejects_stale_confirmation_cancels_jev_and_refreshes_diff_marks(kind: RepoType) {
    let files = repository_fixture(kind);
    files.write("one.rs", b"initial one\n");
    files.write("two.rs", b"initial two\n");
    let state = tempfile::tempdir().unwrap();
    let repository = Repository::discover(files.root())
        .unwrap()
        .with_state_root(state.path());
    let snapshot = complete_repository_snapshot(&repository);
    let checkpoint = ReviewCheckpoint::new(
        snapshot.identity.review_unit().clone(),
        snapshot.identity.snapshot_id(),
    );
    let store = ReviewStore::open(state.path(), files.root()).unwrap();
    for path in ["one.rs", "two.rs"] {
        store
            .mark(
                &checkpoint.review_unit,
                path.as_bytes(),
                &checkpoint.checkpoint,
            )
            .unwrap();
    }
    let active = Arc::new(AtomicBool::new(true));
    let mut worker = Worker {
        repository: repository.clone(),
        tracker: Arc::new(ReviewTracker::new(repository, store.clone())),
        store: store.clone(),
        client: HerdrClient::new(
            state.path().join("unused.sock"),
            "review-reset-test".into(),
            state.path().to_owned(),
        ),
        target: AgentTarget::new(WorkspaceId("private-test".into()), None),
        snapshot: Some(snapshot),
        commands: mpsc::channel().0,
        explore: explore::ExploreRuntime::default(),
        auto_review: Some(active.clone()),
        prompts: comment_service::test_worker(&store).prompt_sender(),
        documents: mpsc::channel().0,
    };
    let (sender, receiver) = crate::runtime::application_message_channel();
    files.write("one.rs", b"changed after the dialog opened\n");
    worker.unreview_all(&checkpoint, &sender);
    assert!(matches!(
        store.load(&checkpoint.review_unit, b"two.rs").unwrap(),
        LoadResult::Reviewed(_)
    ));
    assert!(receiver.try_iter().any(|event| {
        event
            .downcast_ref::<ui_events::ToastRequested>()
            .is_some_and(|toast| toast.kind == toasts::ToastKind::Error)
    }));

    let active = Arc::new(AtomicBool::new(true));
    worker.auto_review = Some(active.clone());
    let current = worker.snapshot.as_ref().unwrap();
    let checkpoint = ReviewCheckpoint::new(
        current.identity.review_unit().clone(),
        current.identity.snapshot_id(),
    );
    worker.unreview_all(&checkpoint, &sender);
    assert!(!active.load(Ordering::Relaxed));
    let refreshed: Vec<_> = receiver
        .try_iter()
        .filter_map(|event| event.downcast_ref::<ReviewStateSaved>().cloned())
        .collect();
    for path in ["one.rs", "two.rs"] {
        assert_eq!(
            store
                .load(&checkpoint.review_unit, path.as_bytes())
                .unwrap(),
            LoadResult::Unreviewed
        );
        assert!(refreshed.iter().any(|event| {
            event.path == path
                && event
                    .result
                    .as_ref()
                    .is_ok_and(|state| state.status == ReviewStatus::Unreviewed)
        }));
    }
}
