//! The session keeps the mark tally of the change right as review marks change during a round:
//! an answer applies its marks, a cancelled answer gives them back, and marks made by hand or
//! by Jev reach it through `marks_changed`.

use review_explore::NotRelevantMark;
use review_explore_page::RoundStage;
use review_explore_tally::{MarkTally, MarkedLines, Marker, PendingLines, Share, WholeFile};
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

#[test]
fn the_page_hears_what_answering_the_waiting_question_adds_as_marks_change() {
    let mut harness = Harness::three_lines();
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    assert!(applied(harness.submit(&access, first_question(&first))));
    let shares = |harness: &Harness| {
        harness
            .page
            .tally()
            .and_then(|tally| tally.gain)
            .map(|gain| (gain.before.percent, gain.after.percent))
    };
    assert_eq!(shares(&harness), Some((0, 33)));

    // A line marked by hand while the question waits counts before and after the answer.
    harness.mark_lines(&[1], &MarkAuthor::Reviewer);
    assert_eq!(shares(&harness), Some((33, 66)));

    // Once the question has its answer, none waits.
    harness.answer("Keep it.");
    assert_eq!(shares(&harness), None);
}

#[test]
fn the_page_hears_the_mark_tally_as_marks_change_during_a_round() {
    let mut harness = Harness::three_lines();
    let marked = |harness: &Harness| harness.page.tally().map(|tally| tally.change.marked);
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    assert!(applied(harness.submit(&access, first_question(&first))));
    // The question waits: the page hears what its answer marks with the question.
    let waiting = harness
        .page
        .tally()
        .expect("the tally comes with the round");
    assert_eq!(waiting.change.pending.not_relevant, 1);
    assert!(waiting.gain.is_some());

    // A run of Jev and a mark by hand reach the page while the question waits.
    harness.mark_lines(&[1], &MarkAuthor::Jev);
    harness.mark_lines(&[2], &MarkAuthor::Reviewer);
    assert_eq!(
        marked(&harness),
        Some(MarkedLines {
            jev: 1,
            by_hand: 1,
            ..MarkedLines::default()
        })
    );

    // The answer applies its marks, which the page hears with the agent at work.
    harness.answer("Keep it.");
    assert_eq!(
        marked(&harness),
        Some(MarkedLines {
            jev: 1,
            not_relevant: 1,
            by_hand: 1,
            ..MarkedLines::default()
        })
    );
    assert_eq!(harness.page.tally().and_then(|tally| tally.gain), None);
}

#[test]
fn implementing_at_the_drawing_board_reopens_every_line_the_round_marked_and_gives_none_back() {
    let mut harness = Harness::three_lines();
    harness.mark_lines(&[1], &MarkAuthor::Reviewer);
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    assert!(applied(harness.submit(&access, first_question(&first))));
    let (answered, access) = harness.answer("Keep it.");
    // The second answer takes over the second line, which the reviewer marked by hand.
    let Operation::SubmitQuestion(mut takes_over) = second_question(&answered) else {
        unreachable!("a question");
    };
    takes_over.reopened = takes_over.reviewed.clone();
    assert!(applied(
        harness.submit(&access, Operation::SubmitQuestion(takes_over))
    ));
    let (answered, access) = harness.answer("Keep it.");
    assert!(applied(harness.submit(&access, question(&answered, 3))));
    let request = harness.request(Some(AnswerInput {
        drawing_board: true,
        ..AnswerInput::default()
    }));
    let access = harness.turn(&request);
    let request = harness.exploration().retry_request().unwrap().clone();
    assert!(applied(
        harness.submit(&access, drawing_board_conclusion(&request))
    ));
    // Until Implement, Cancel answer may still take the drawing board back: the marks stay.
    assert_eq!(harness.session.mark_tally().change.left, 1);
    let conclusion = harness.conclusion_request();

    let implemented = harness.implement_on_page(&conclusion, None, "One queue per conversation.");

    assert_eq!(implemented, Ok(()));
    let tally = harness.session.mark_tally();
    assert_eq!(tally.change.marked, MarkedLines::default());
    assert_eq!(tally.change.left, 3);
}

#[test]
fn implementing_any_other_conclusion_keeps_the_round_s_marks() {
    let mut harness = Harness::three_lines();
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    assert!(applied(harness.submit(&access, first_question(&first))));
    let (answered, access) = harness.answer("Keep it.");
    assert!(applied(
        harness.submit(&access, conclusion(&answered, CONCLUSION))
    ));
    let conclusion = harness.conclusion_request();

    let implemented = harness.implement_on_page(&conclusion, None, "Add a regression test.");

    assert_eq!(implemented, Ok(()));
    assert_eq!(harness.session.mark_tally().change.marked.not_relevant, 1);
}

#[test]
fn an_answer_marks_the_files_whose_diff_did_not_change_since_the_round_started() {
    let mut harness = Harness::three_lines();
    harness.files.write("edited.rs", b"alpha\nbeta\n");
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    let Operation::SubmitQuestion(mut update) = question(&first, 1) else {
        unreachable!("a question");
    };
    update.not_relevant = ["reviewed.rs", "edited.rs"]
        .into_iter()
        .map(|path| {
            serde_json::from_value(serde_json::json!({
                "path": path, "side": "new", "lines": {"first_line": 1, "last_line": 2},
                "reason": "follows_code"
            }))
            .unwrap()
        })
        .collect();
    assert!(applied(
        harness.submit(&access, Operation::SubmitQuestion(update))
    ));
    // A comment above the lines the question names moves them down.
    harness.files.write("edited.rs", b"// note\nalpha\nbeta\n");
    harness.mark_lines(&[], &MarkAuthor::Reviewer);

    let waiting = harness.session.mark_tally();
    let pending = |tally: &MarkTally, path: &str| {
        tally
            .files
            .iter()
            .find(|file| file.path == path)
            .map(|file| file.tally.pending.not_relevant)
    };
    assert_eq!(pending(&waiting, "reviewed.rs"), Some(2));
    assert_eq!(pending(&waiting, "edited.rs"), Some(0));

    harness.answer("Keep it.");

    let answered = harness.session.mark_tally();
    let marked = |path: &str| {
        answered
            .files
            .iter()
            .find(|file| file.path == path)
            .map(|file| file.tally.marked.not_relevant)
    };
    assert_eq!(marked("reviewed.rs"), Some(2));
    assert_eq!(marked("edited.rs"), Some(0));
    let problem = "edited.rs changed since this round started; its lines were not marked";
    let saved = harness.saved();
    let marks = saved.marks.get(&first.request).expect("the answer's marks");
    assert_eq!(marks.problem.as_deref(), Some(problem));
    let RoundStage::AgentWorking {
        answer: Some(sent), ..
    } = harness.page.stage()
    else {
        panic!("the page shows {:?}", harness.page.stage());
    };
    assert_eq!(sent.marks_problem.as_deref(), Some(problem));
}

#[test]
fn a_turn_written_while_a_file_had_changed_marks_nothing_there_once_it_is_back() {
    let mut harness = Harness::three_lines();
    harness.files.write("edited.rs", b"alpha\nbeta\n");
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    assert!(applied(harness.submit(&access, question(&first, 1))));
    let (answered, access) = harness.answer("Keep it.");
    // The agent reads edited.rs while a comment moves its lines, and names them so.
    harness.files.write("edited.rs", b"// note\nalpha\nbeta\n");
    let Operation::SubmitQuestion(mut update) = question(&answered, 2) else {
        unreachable!("a question");
    };
    update.reviewed = ["reviewed.rs", "edited.rs"]
        .into_iter()
        .map(|path| {
            serde_json::from_value(serde_json::json!({
                "path": path, "side": "new", "lines": {"first_line": 2, "last_line": 3}
            }))
            .unwrap()
        })
        .collect();
    assert!(applied(
        harness.submit(&access, Operation::SubmitQuestion(update))
    ));
    // Then the comment goes: the diff of edited.rs is the round's again.
    harness.files.write("edited.rs", b"alpha\nbeta\n");
    harness.mark_lines(&[], &MarkAuthor::Reviewer);

    let (second, access) = harness.answer("Keep them.");

    let marked = |path: &str| {
        harness
            .session
            .mark_tally()
            .files
            .iter()
            .find(|file| file.path == path)
            .map(|file| file.tally.marked.answers)
    };
    assert_eq!(marked("reviewed.rs"), Some(2));
    assert_eq!(marked("edited.rs"), Some(0));
    // The turn after the answer says so above its question.
    assert!(applied(harness.submit(&access, question(&second, 3))));
    let stage = harness.page.stage();
    assert_eq!(
        stage
            .response()
            .and_then(|response| response.marks_problem.as_deref()),
        Some("edited.rs changed since this round started; its lines were not marked")
    );
}

#[test]
fn a_prompt_written_while_a_file_had_changed_marks_nothing_there_once_it_is_back() {
    let mut harness = Harness::three_lines();
    harness.files.write("edited.rs", b"alpha\nbeta\n");
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    assert!(applied(harness.submit(&access, question(&first, 1))));
    // The prompt after the answer numbers edited.rs with the comment.
    harness.files.write("edited.rs", b"// note\nalpha\nbeta\n");
    let (answered, access) = harness.answer("Keep it.");
    // The comment goes before the agent submits.
    harness.files.write("edited.rs", b"alpha\nbeta\n");
    let Operation::SubmitQuestion(mut update) = question(&answered, 2) else {
        unreachable!("a question");
    };
    update.reviewed = vec![
        serde_json::from_value(serde_json::json!({
            "path": "edited.rs", "side": "new", "lines": {"first_line": 1, "last_line": 2}
        }))
        .unwrap(),
    ];
    assert!(applied(
        harness.submit(&access, Operation::SubmitQuestion(update))
    ));
    harness.mark_lines(&[], &MarkAuthor::Reviewer);

    harness.answer("Keep them.");

    let edited = harness
        .session
        .mark_tally()
        .files
        .into_iter()
        .find(|file| file.path == "edited.rs")
        .map(|file| file.tally.marked.answers);
    assert_eq!(edited, Some(0));
}
