//! The session keeps the mark tally of the change right as review marks change during a round:
//! an answer applies its marks, a cancelled answer gives them back, and marks made by hand or
//! by Jev reach it through `marks_changed`.

use review_explore::NotRelevantMark;
use review_explore_tally::{MarkedLines, Marker, PendingLines, Share, WholeFile};
use review_hunks::LineSelection;
use review_types::MarkAuthor;

use super::*;

impl Harness {
    /// The reviewer's change holds three added lines.
    fn three_lines() -> Self {
        let harness = Self::start();
        harness.files.write("reviewed.rs", b"one\ntwo\nthree\n");
        harness
    }

    /// Mark the added lines `added` (zero-based) as `author`, then tell the session as the
    /// reviewer's worker does.
    fn mark_lines(&mut self, added: &[u32], author: &MarkAuthor) {
        let tracker = review_state::ReviewTracker::new(self.repository.clone(), self.store.clone());
        let snapshot = complete_repository_snapshot(&self.repository);
        let selection = LineSelection {
            removed: [].into(),
            added: added.iter().copied().collect(),
        };
        tracker
            .accept_lines(&snapshot, &snapshot.files[0], &selection, author)
            .unwrap();
        let states = tracker.statuses(&snapshot).unwrap();
        self.session.marks_changed(&snapshot, &states);
    }
}

/// The first question, which finds the first line not relevant once it is answered.
fn first_question(request: &TurnRequest) -> Operation {
    let Operation::SubmitQuestion(mut update) = question(request, 1) else {
        unreachable!("a question");
    };
    update.not_relevant = vec![
        serde_json::from_value::<NotRelevantMark>(serde_json::json!({
            "path": "reviewed.rs", "side": "new", "lines": {"first_line": 1, "last_line": 1},
            "reason": "follows_code"
        }))
        .unwrap(),
    ];
    Operation::SubmitQuestion(update)
}

/// The second question, which marks the second line reviewed once it is answered.
fn second_question(request: &TurnRequest) -> Operation {
    let Operation::SubmitQuestion(mut update) = question(request, 2) else {
        unreachable!("a question");
    };
    update.reviewed = vec![
        serde_json::from_value(serde_json::json!({
            "path": "reviewed.rs", "side": "new", "lines": {"first_line": 2, "last_line": 2}
        }))
        .unwrap(),
    ];
    Operation::SubmitQuestion(update)
}

#[test]
fn the_mark_tally_follows_an_answer_and_its_cancellation() {
    let mut harness = Harness::three_lines();
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    assert!(applied(harness.submit(&access, first_question(&first))));

    // The question waits: its answer would mark one line not relevant.
    let waiting = harness.session.mark_tally();
    assert_eq!(waiting.change.changed.lines_added, 3);
    assert_eq!(waiting.change.marked, MarkedLines::default());
    assert_eq!(
        waiting.change.pending,
        PendingLines {
            reviewed: 0,
            not_relevant: 1
        }
    );
    assert!(waiting.files[0].cited);
    let gain = waiting.gain.expect("a question waits");
    assert_eq!((gain.before.percent, gain.after.percent), (0, 33));

    // The answer applies the marks; the agent works, and no question waits.
    let (answered, access) = harness.answer("Keep it.");
    let working = harness.session.mark_tally();
    assert_eq!(working.change.marked.not_relevant, 1);
    assert_eq!(working.change.left, 2);
    assert_eq!(working.gain, None);

    // The next question would mark another line reviewed.
    assert!(applied(harness.submit(&access, second_question(&answered))));
    let next = harness.session.mark_tally();
    assert_eq!(next.change.marked.not_relevant, 1);
    assert_eq!(next.change.pending.reviewed, 1);
    assert_eq!(
        next.gain.unwrap().after,
        Share {
            marked: 2,
            changed: 3,
            percent: 66
        }
    );

    // Cancelling the answer gives its marks back, and the first question waits again.
    let answer = answered.answer.unwrap().id;
    harness
        .session
        .handle(Input::Command(Command::CancelAnswer(answer)));
    assert!(
        harness
            .next::<ui_events::ExploreAnswerCancelled>()
            .result
            .is_ok()
    );
    let cancelled = harness.session.mark_tally();
    assert_eq!(cancelled.change.marked, MarkedLines::default());
    assert_eq!(cancelled.change.pending.not_relevant, 1);
    assert_eq!(cancelled.change.left, 2);
}

#[test]
fn marks_by_hand_and_by_jev_reach_the_mark_tally_through_marks_changed() {
    let mut harness = Harness::three_lines();

    harness.mark_lines(&[0], &MarkAuthor::Jev);
    harness.mark_lines(&[1], &MarkAuthor::Reviewer);

    let tally = harness.session.mark_tally();
    assert_eq!(
        tally.change.marked,
        MarkedLines {
            jev: 1,
            by_hand: 1,
            ..MarkedLines::default()
        }
    );
    assert_eq!(tally.change.left, 1);
    assert_eq!(tally.gain, None);
}

#[test]
fn a_binary_file_is_tallied_whole_as_the_tracker_marks_it() {
    let mut harness = Harness::three_lines();
    harness
        .files
        .write("logo.png", b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0");
    let tracker =
        review_state::ReviewTracker::new(harness.repository.clone(), harness.store.clone());
    let snapshot = complete_repository_snapshot(&harness.repository);
    let logo = snapshot
        .files
        .iter()
        .find(|file| file.review_path().display() == "logo.png")
        .expect("the binary file is a changed file");
    tracker
        .mark(&snapshot, logo, &MarkAuthor::Reviewer)
        .unwrap();
    let states = tracker.statuses(&snapshot).unwrap();

    harness.session.marks_changed(&snapshot, &states);

    let tally = harness.session.mark_tally();
    let logo = tally
        .files
        .iter()
        .find(|file| file.path == "logo.png")
        .unwrap();
    assert_eq!(
        logo.whole,
        Some(WholeFile {
            marked_by: Some(Marker::ByHand),
            answering: None
        })
    );
    assert_eq!(logo.tally.left, 0);
    assert_eq!(tally.change.changed.lines_added, 3);
}

#[test]
fn cancelling_an_answer_gives_a_binary_file_back_to_who_had_marked_it() {
    let mut harness = Harness::three_lines();
    harness
        .files
        .write("logo.png", b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0");
    let tracker =
        review_state::ReviewTracker::new(harness.repository.clone(), harness.store.clone());
    let snapshot = complete_repository_snapshot(&harness.repository);
    let logo = snapshot
        .files
        .iter()
        .find(|file| file.review_path().display() == "logo.png")
        .unwrap();
    tracker.mark(&snapshot, logo, &MarkAuthor::Jev).unwrap();
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    assert!(applied(harness.submit(&access, question(&first, 1))));
    let (answered, access) = harness.answer("Keep it.");
    // The answer takes the binary file over from Jev.
    let Operation::SubmitConclusion(mut conclusion) = conclusion(&answered, CONCLUSION) else {
        unreachable!("a conclusion");
    };
    let whole: review_explore::CodeLocation = serde_json::from_value(serde_json::json!({
        "path": "logo.png", "side": "new", "lines": null
    }))
    .unwrap();
    conclusion.reviewed = vec![whole.clone()];
    conclusion.reopened = vec![whole];
    assert!(applied(
        harness.submit(&access, Operation::SubmitConclusion(conclusion))
    ));
    let marked_by = |harness: &Harness| {
        harness
            .session
            .mark_tally()
            .files
            .iter()
            .find(|file| file.path == "logo.png")
            .and_then(|file| file.whole)
            .and_then(|whole| whole.marked_by)
    };
    assert_eq!(marked_by(&harness), Some(Marker::Answer));

    let answer = answered.answer.unwrap().id;
    harness
        .session
        .handle(Input::Command(Command::CancelAnswer(answer)));
    assert!(
        harness
            .next::<ui_events::ExploreAnswerCancelled>()
            .result
            .is_ok()
    );

    assert_eq!(marked_by(&harness), Some(Marker::Jev));
    let review_store::LoadResult::Reviewed(record) =
        harness.store.load(&harness.unit, b"logo.png").unwrap()
    else {
        panic!("the binary file is marked again");
    };
    assert_eq!(record.author, MarkAuthor::Jev);
    assert!(
        record.partial.is_none(),
        "a whole-file mark, not a mark of text lines"
    );
}
