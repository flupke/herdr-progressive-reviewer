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

/// Classifies these lines of the first file insignificant on both sides.
struct InsignificantLines(&'static [u32]);

/// Classifies line 3 of the first file insignificant on both sides.
const LINE_THREE: InsignificantLines = InsignificantLines(&[3]);

impl review_significance::SignificanceClassifier for InsignificantLines {
    fn rubric(&self) -> &'static str {
        "chosen lines"
    }

    fn plan(
        &self,
        _: &review_explore::Comparison,
        _: &dyn Fn(&str) -> bool,
    ) -> review_significance::SignificancePlan {
        let lines = self.0;
        review_significance::SignificancePlan::new(lines.len(), move |record| {
            lines.iter().all(|&line| {
                record(review_significance::SignificanceResult {
                    id: format!("line-{line}"),
                    units: [
                        review_explore::SourceSide::Old,
                        review_explore::SourceSide::New,
                    ]
                    .into_iter()
                    .map(|side| review_significance::ChangeUnit::Lines {
                        file: 0,
                        side,
                        first: line,
                        end: line + 1,
                    })
                    .collect(),
                    outcome: review_significance::Significance::Insignificant,
                    model: None,
                    rubric: "chosen lines".into(),
                    criterion: String::new(),
                    input_references: vec![],
                    omissions: vec![],
                    probabilities: std::collections::BTreeMap::default(),
                    confidence: None,
                    error: None,
                })
            })
        })
    }
}

#[test_case::test_case(RepoType::Git; "git")]
#[test_case::test_case(RepoType::Jj; "jj")]
fn automatic_review_announces_the_hunks_it_marks(kind: RepoType) {
    let files = repository_fixture(kind);
    files.write("file.rs", &text(&[]));
    files.new_change("review");
    files.write("file.rs", &text(&[(3, "three"), (20, "twenty")]));
    let mut fixture = EffectsFixture::start(files, |setup| {
        setup.jev = review_significance::JevClassifier::enabled(std::sync::Arc::new(LINE_THREE));
    });
    let checkpoint = fixture.refreshed_checkpoint();

    fixture.perform([Action::Repository(RepositoryAction::AutoReview(checkpoint))]);

    let mut events = fixture.events_until::<ui_events::ToastRequested>();
    events.extend(fixture.events_until::<ui_events::ToastRequested>());
    let toast = events
        .iter()
        .filter_map(|event| event.downcast_ref::<ui_events::ToastRequested>())
        .next_back()
        .unwrap();
    assert!(toast.text.contains("1 hunk reviewed"), "{}", toast.text);
    let saved = events
        .iter()
        .filter_map(|event| event.downcast_ref::<ReviewStateSaved>())
        .collect::<Vec<_>>();
    assert!(
        saved.iter().any(|saved| saved.path == "file.rs"
            && saved.result.map(|state| state.status) == Ok(ReviewStatus::PartiallyReviewed)),
        "{saved:?}"
    );
}

#[test_case::test_case(RepoType::Git; "git")]
#[test_case::test_case(RepoType::Jj; "jj")]
fn an_explore_kickoff_waits_for_jev_to_mark_what_it_dismisses(kind: RepoType) {
    let files = repository_fixture(kind);
    files.write("file.rs", &text(&[]));
    files.new_change("review");
    files.write("file.rs", &text(&[(3, "three"), (20, "twenty")]));
    let mut fixture = EffectsFixture::start(files, |setup| {
        setup.jev = review_significance::JevClassifier::enabled(std::sync::Arc::new(LINE_THREE));
    });
    fixture.refreshed_checkpoint();
    fixture.perform([Action::Explore(review_explore::Command::Start)]);
    let comparison = fixture
        .wait_for::<ui_events::ExploreCaptured>()
        .result
        .unwrap();
    let mut exploration = review_explore::Exploration::new(comparison);
    let kickoff = exploration.request(None, None).unwrap();

    fixture.perform([Action::Explore(review_explore::Command::Turn(Box::new(
        kickoff,
    )))]);

    let events = fixture.events_until::<ui_events::ExplorePosted>();
    let marked = events.iter().any(|event| {
        event
            .downcast_ref::<ReviewStateSaved>()
            .is_some_and(|saved| {
                saved.path == "file.rs"
                    && saved.result.map(|state| state.status) == Ok(ReviewStatus::PartiallyReviewed)
            })
    });
    assert!(
        marked,
        "Jev marked line three before the kickoff was posted"
    );
}

#[test_case::test_case(RepoType::Git; "git")]
#[test_case::test_case(RepoType::Jj; "jj")]
fn a_round_started_from_the_page_waits_for_jev_to_mark_what_it_dismisses(kind: RepoType) {
    let files = repository_fixture(kind);
    files.write("file.rs", &text(&[]));
    files.new_change("review");
    files.write("file.rs", &text(&[(3, "three"), (20, "twenty")]));
    let mut page = None;
    let mut fixture = EffectsFixture::start(files, |setup| {
        setup.jev = review_significance::JevClassifier::enabled(std::sync::Arc::new(LINE_THREE));
        page = Some(setup.page.subscribe());
    });
    fixture.refreshed_checkpoint();
    let (reply, replied) = review_explore_page::CommandReply::channel();
    // The start the page offers.
    let review_explore_page::RoundStage::NoRound { start } = page.unwrap().stage() else {
        panic!("the page offers no start");
    };

    fixture
        .effects
        .explore_inbox()
        .deliver(review_explore_session::Input::Page {
            command: review_explore_page::PageCommand::Start {
                challenger: true,
                start,
            },
            reply,
        });

    assert_eq!(replied.blocking_recv(), Ok(Ok(())));
    let events = fixture.events_until::<ui_events::ExplorePosted>();
    let marked = events.iter().any(|event| {
        event
            .downcast_ref::<ReviewStateSaved>()
            .is_some_and(|saved| {
                saved.path == "file.rs"
                    && saved.result.map(|state| state.status) == Ok(ReviewStatus::PartiallyReviewed)
            })
    });
    assert!(
        marked,
        "Jev marked line three before the kickoff was posted"
    );
    let posted = events
        .iter()
        .find_map(|event| event.downcast_ref::<ui_events::ExplorePosted>())
        .unwrap();
    assert!(posted.request.is_kickoff() && posted.request.challenger);
}

#[test]
fn marking_every_changed_file_reviewed_turns_the_explore_starts_off_until_one_is_unmarked() {
    let mut fixture = fixture(RepoType::Jj);
    fixture.refreshed_checkpoint();

    fixture.perform([Action::Repository(RepositoryAction::SetReviewed {
        path: "file.rs".to_owned(),
        reviewed: true,
    })]);
    assert_eq!(
        fixture.wait_for::<ui_events::ExploreStartBlock>().0,
        Some(review_explore::StartBlock::NothingToReview)
    );

    fixture.perform([Action::Repository(RepositoryAction::SetReviewed {
        path: "file.rs".to_owned(),
        reviewed: false,
    })]);
    assert_eq!(fixture.wait_for::<ui_events::ExploreStartBlock>().0, None);
}

#[test]
fn a_kickoff_is_not_sent_once_jev_marked_every_changed_line() {
    let files = repository_fixture(RepoType::Jj);
    files.write("file.rs", &text(&[]));
    files.new_change("review");
    files.write("file.rs", &text(&[(3, "three"), (20, "twenty")]));
    let mut fixture = EffectsFixture::start(files, |setup| {
        setup.jev = review_significance::JevClassifier::enabled(std::sync::Arc::new(
            InsignificantLines(&[3, 20]),
        ));
    });
    fixture.refreshed_checkpoint();
    fixture.perform([Action::Explore(review_explore::Command::Start)]);
    let comparison = fixture
        .wait_for::<ui_events::ExploreCaptured>()
        .result
        .unwrap();
    let kickoff = review_explore::Exploration::new(comparison)
        .request(None, None)
        .unwrap();

    fixture.perform([Action::Explore(review_explore::Command::Turn(Box::new(
        kickoff,
    )))]);

    let posted = fixture.wait_for::<ui_events::ExplorePosted>();
    assert_eq!(
        posted.result.err().as_deref(),
        Some(review_explore::StartBlock::NothingToReview.reason())
    );
}
