use review_hunks::{HunkMark, HunkSpan};
use review_repository::repository::RepoType;
use review_source::ReviewCheckpoint;
use review_state::ReviewStatus;
use review_test_support::repository_fixture;
use review_ui::{Action, RepositoryAction};
use ui_events::ReviewStateSaved;

use crate::runtime::effects::fixture::EffectsFixture;

fn text(edits: &[(usize, &str)]) -> Vec<u8> {
    let mut lines = (1..=30)
        .map(|line| format!("line {line}"))
        .collect::<Vec<_>>();
    for (line, replacement) in edits {
        (*replacement).clone_into(&mut lines[*line - 1]);
    }
    lines
        .join("\n")
        .into_bytes()
        .into_iter()
        .chain([b'\n'])
        .collect()
}

fn fixture(kind: RepoType) -> EffectsFixture {
    let files = repository_fixture(kind);
    files.write("file.rs", &text(&[]));
    files.new_change("review");
    files.write("file.rs", &text(&[(3, "three"), (20, "twenty")]));
    EffectsFixture::start(files, |_| {})
}

fn accept_line_three(checkpoint: ReviewCheckpoint) -> Action {
    Action::Repository(RepositoryAction::SetHunkReviewed {
        review_checkpoint: checkpoint,
        path: "file.rs".to_owned(),
        mark: HunkMark::Review(HunkSpan {
            old: 2..3,
            new: 2..3,
        }),
    })
}

#[test_case::test_case(RepoType::Git; "git")]
#[test_case::test_case(RepoType::Jj; "jj")]
fn accepting_a_hunk_saves_a_partial_review(kind: RepoType) {
    let mut fixture = fixture(kind);
    let checkpoint = fixture.refreshed_checkpoint();

    fixture.perform([accept_line_three(checkpoint)]);

    let saved = fixture.wait_for::<ReviewStateSaved>();
    assert_eq!(saved.path, "file.rs");
    assert_eq!(
        saved.result.map(|state| state.status),
        Ok(ReviewStatus::PartiallyReviewed)
    );
}

#[test]
fn a_hunk_from_an_older_comparison_is_refused() {
    let mut fixture = fixture(RepoType::Jj);
    let checkpoint = fixture.refreshed_checkpoint();

    fixture.perform([accept_line_three(ReviewCheckpoint::new(
        checkpoint.review_unit.clone(),
        "0".repeat(40),
    ))]);

    let toast = fixture.wait_for::<ui_events::ToastRequested>();
    assert_eq!(toast.kind, toasts::ToastKind::Error);
    assert_eq!(fixture.wait_for::<ReviewStateSaved>().result, Err(()));
    assert_eq!(
        fixture
            .store
            .load(&checkpoint.review_unit, b"file.rs")
            .unwrap(),
        review_store::LoadResult::Unreviewed
    );
}
