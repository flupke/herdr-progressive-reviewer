use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

use review_explore::Comparison;

use review_repository::repository::RepoType;
use review_significance::{JevClassifier, SignificanceClassifier, SignificancePlan};
use review_state::ReviewStatus;
use review_store::LoadResult;
use review_test_support::repository_fixture;
use review_ui::{Action, RepositoryAction};

use super::*;
use crate::runtime::effects::fixture::EffectsFixture;

/// Holds classification open until the test releases it.
struct GatedClassifier(Mutex<Option<Receiver<()>>>);

impl GatedClassifier {
    fn new() -> (Self, Sender<()>) {
        let (release, gate) = mpsc::channel();
        (Self(Mutex::new(Some(gate))), release)
    }
}

impl SignificanceClassifier for GatedClassifier {
    fn rubric(&self) -> &'static str {
        "gated"
    }

    fn plan(&self, _: &Comparison, _: &dyn Fn(&str) -> bool) -> SignificancePlan {
        let gate = self.0.lock().unwrap().take();
        SignificancePlan::new(0, move |_| {
            if let Some(gate) = gate {
                let _ = gate.recv();
            }
            true
        })
    }
}

fn toast(fixture: &mut EffectsFixture) -> ui_events::ToastRequested {
    fixture.wait_for::<ui_events::ToastRequested>()
}

#[test_case::test_case(RepoType::Git; "git")]
#[test_case::test_case(RepoType::Jj; "jj")]
fn bulk_reset_rejects_stale_confirmation_cancels_jev_and_refreshes_diff_marks(kind: RepoType) {
    let files = repository_fixture(kind);
    files.write("one.rs", b"initial one\n");
    files.write("two.rs", b"initial two\n");
    let (classifier, release) = GatedClassifier::new();
    let mut fixture = EffectsFixture::start(files, |setup| {
        setup.jev = JevClassifier::enabled(Arc::new(classifier));
    });
    let checkpoint = fixture.refreshed_checkpoint();
    for path in ["one.rs", "two.rs"] {
        fixture
            .store
            .mark(
                &checkpoint.review_unit,
                path.as_bytes(),
                &checkpoint.checkpoint,
                &review_types::MarkAuthor::Reviewer,
            )
            .unwrap();
    }
    fixture
        .files
        .write("one.rs", b"changed after the dialog opened\n");

    fixture.perform([Action::Repository(RepositoryAction::UnreviewAll(
        checkpoint.clone(),
    ))]);

    assert_eq!(toast(&mut fixture).kind, toasts::ToastKind::Error);
    assert!(matches!(
        fixture
            .store
            .load(&checkpoint.review_unit, b"two.rs")
            .unwrap(),
        LoadResult::Reviewed(_)
    ));

    let checkpoint = fixture.refreshed_checkpoint();
    fixture.perform([Action::Repository(RepositoryAction::AutoReview(
        checkpoint.clone(),
    ))]);
    assert!(toast(&mut fixture).text.starts_with("Jev: classifying"));
    fixture.perform([Action::Repository(RepositoryAction::UnreviewAll(
        checkpoint.clone(),
    ))]);
    let events = fixture.events_until::<ui_events::ToastRequested>();
    let refreshed: Vec<_> = events
        .iter()
        .filter_map(|event| event.downcast_ref::<ReviewStateSaved>())
        .collect();
    for path in ["one.rs", "two.rs"] {
        assert_eq!(
            fixture
                .store
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
    drop(release);
    assert!(
        toast(&mut fixture)
            .text
            .starts_with("Automatic review cancelled"),
        "the reset cancels the running Jev review"
    );
}
