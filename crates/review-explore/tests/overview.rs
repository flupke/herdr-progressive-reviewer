//! The overview of saved rounds, read from synthetic fixtures in `overview/`, each a round as
//! the store saves it, over a change to `store.rs`:
//!
//! - `kickoff.json`: the round's first turn is on its way.
//! - `working.json`: question 1 ("cache", two-way door) answered with the recommended choice;
//!   question 2 ("lock", one-way door) answered with the recommendation after a first pick of
//!   another choice, and on its way.
//! - `cancelled.json`: as `working.json`, then the agent asked question 3 and the reviewer
//!   cancelled the answer to question 2.
//! - `quiz.json`: as `working.json`, then question 3 ("flush") answered with None of the above,
//!   clarified as its version 2 and answered with a comment only; question 4 ("crash")
//!   answered with None of the above after a first pick; the conclusion with a quiz of three
//!   items, the first answered correctly. Answers 1 to 3 applied review marks.
//! - `concluded.json`: as `quiz.json`, with the two other items answered wrongly.
//! - `replied.json`: as `concluded.json`, then the reviewer replied to the conclusion and the
//!   agent asked question 5 ("readers").
//! - `reasked.json`: question 1 answered with None of the above, question 2 answered, then
//!   version 2 of question 1 waiting.
//! - `legacy.json`: a round saved before rounds had a design, a quiz, first picks and review
//!   marks: one question answered with a choice and a comment, then a conclusion.
use review_explore::*;

/// The round saved in the fixture `name`.
fn saved(name: &str) -> ExploreRound {
    let path = format!("{}/tests/overview/{name}.json", env!("CARGO_MANIFEST_DIR"));
    let mut file: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    serde_json::from_value(file["value"].take()).unwrap()
}

fn step(step: Step, state: StepState) -> RailStep {
    RailStep { step, state }
}

fn question(number: usize) -> Step {
    Step::Question { number }
}

fn current(working: bool) -> StepState {
    StepState::Current { working }
}

const LATER_QUIZ: Step = Step::Quiz {
    stage: QuizStage::Later,
};

/// The request of the turn `round` waits for.
fn pending(round: &ExploreRound) -> String {
    round.exploration.pending_request().unwrap().request.clone()
}

fn kept(choice: Option<&str>, comment: &str, tag: Option<DecisionTag>) -> KeptAnswer {
    KeptAnswer {
        choice: choice.map(Into::into),
        comment: comment.into(),
        tag,
    }
}

fn numbers(overview: &RoundOverview) -> (Vec<usize>, Vec<usize>) {
    (
        overview.decisions.iter().map(|d| d.number).collect(),
        overview.earlier.iter().map(|e| e.number).collect(),
    )
}

#[test]
fn a_concluded_round_has_every_step_done_up_to_its_conclusion() {
    let overview = RoundOverview::of(&saved("concluded"), None);

    assert_eq!(
        overview.rail,
        vec![
            step(Step::Design, StepState::Done),
            step(question(1), StepState::Done),
            step(question(2), StepState::Done),
            step(question(3), StepState::Done),
            step(question(4), StepState::Done),
            step(
                Step::Quiz {
                    stage: QuizStage::Scored {
                        correct: 1,
                        answered: 3,
                        items: 3
                    }
                },
                StepState::Done
            ),
            step(Step::Conclusion, StepState::Current { working: false }),
        ]
    );
}

#[test]
fn the_quiz_is_current_until_the_reviewer_answers_every_item() {
    let overview = RoundOverview::of(&saved("quiz"), None);

    assert_eq!(
        overview.rail[5..],
        [
            step(
                Step::Quiz {
                    stage: QuizStage::Running { item: 2, items: 3 }
                },
                current(false)
            ),
            step(Step::Conclusion, StepState::Later),
        ]
    );
    assert_eq!(overview.title, TabTitle::Conclusion);
}

#[test]
fn a_skipped_quiz_scores_the_items_answered() {
    let mut round = saved("quiz");
    let conclusion = round.exploration.conclusion_request().unwrap().to_owned();
    round
        .exploration
        .answer_quiz(&conclusion, QuizResponse::Skip)
        .unwrap();

    let overview = RoundOverview::of(&round, None);

    let stage = QuizStage::Scored {
        correct: 1,
        answered: 1,
        items: 3,
    };
    assert_eq!(
        overview.rail[5],
        step(Step::Quiz { stage }, StepState::Done)
    );
}

#[test]
fn the_answered_question_is_working_while_the_session_delivers_its_turn() {
    let round = saved("working");

    let overview = RoundOverview::of(&round, Some(&pending(&round)));

    assert_eq!(
        overview.rail,
        vec![
            step(Step::Design, StepState::Done),
            step(question(1), StepState::Done),
            step(question(2), current(true)),
            step(LATER_QUIZ, StepState::Later),
            step(Step::Conclusion, StepState::Later),
        ]
    );
    assert_eq!(overview.title, TabTitle::AgentWorking);
    // The answer on its way is a decision; the question is not behind the round yet.
    assert_eq!(numbers(&overview), (vec![1, 2], vec![1]));
}

#[test]
fn a_pending_turn_the_session_does_not_deliver_needs_a_retry() {
    let round = saved("working");

    let overview = RoundOverview::of(&round, None);

    assert_eq!(overview.rail[2], step(question(2), current(false)));
    assert_eq!(overview.title, TabTitle::RetryNeeded);
}

#[test]
fn a_failed_turn_needs_a_retry() {
    let mut round = saved("working");
    let request = pending(&round);
    assert!(
        round
            .exploration
            .failed(&request, "The agent's pane closed")
    );

    let overview = RoundOverview::of(&round, Some(&request));

    assert_eq!(overview.rail[2], step(question(2), current(false)));
    assert_eq!(overview.title, TabTitle::RetryNeeded);
}

#[test]
fn a_cancelled_answer_returns_its_question_to_the_reviewer() {
    let overview = RoundOverview::of(&saved("cancelled"), None);

    // The question the agent asked after the cancelled answer is gone.
    assert_eq!(
        overview.rail,
        vec![
            step(Step::Design, StepState::Done),
            step(question(1), StepState::Done),
            step(question(2), current(false)),
            step(LATER_QUIZ, StepState::Later),
            step(Step::Conclusion, StepState::Later),
        ]
    );
    assert_eq!(overview.title, TabTitle::YourTurn { question: 2 });
    assert_eq!(numbers(&overview), (vec![1], vec![1]));
}

#[test]
fn cancelling_the_answer_on_its_way_returns_its_question_to_the_reviewer() {
    let mut round = saved("working");
    let answer = round.exploration.answers.last().unwrap().id.clone();
    round.cancel_answer(&answer).unwrap();

    let overview = RoundOverview::of(&round, None);

    assert_eq!(overview.rail[2], step(question(2), current(false)));
    assert_eq!(overview.title, TabTitle::YourTurn { question: 2 });
    assert_eq!(numbers(&overview), (vec![1], vec![1]));
}

#[test]
fn the_design_is_current_while_the_agent_works_on_the_first_turn() {
    let round = saved("kickoff");

    let overview = RoundOverview::of(&round, Some(&pending(&round)));

    assert_eq!(
        overview.rail,
        vec![
            step(Step::Design, current(true)),
            step(LATER_QUIZ, StepState::Later),
            step(Step::Conclusion, StepState::Later),
        ]
    );
    assert_eq!(overview.title, TabTitle::AgentWorking);
    assert_eq!(numbers(&overview), (vec![], vec![]));
}

#[test]
fn the_decisions_tag_the_kept_choice() {
    let overview = RoundOverview::of(&saved("concluded"), None);

    let decisions: Vec<_> = overview
        .decisions
        .iter()
        .map(|decision| {
            (
                decision.number,
                decision.question.as_str(),
                &decision.answer,
            )
        })
        .collect();
    assert_eq!(
        decisions,
        vec![
            (
                1,
                "Keep a write cache in front of the store?",
                &kept(Some("Keep the cache"), "", Some(DecisionTag::AsRecommended))
            ),
            // The reviewer first picked "A queue per writer", then took the recommendation.
            (
                2,
                "How do writers share the cache?",
                &kept(
                    Some("One mutex"),
                    "",
                    Some(DecisionTag::ChangedAfterFirstPick)
                )
            ),
            // Clarified once: the decision is the answer to the second version, a comment.
            (
                3,
                "When does the cache flush: on a timer or on size?",
                &kept(None, "A timer of five seconds, and on close.", None)
            ),
            (
                4,
                "What may a crash lose?",
                &kept(
                    Some("None of the above"),
                    "Lose nothing that was acknowledged.",
                    Some(DecisionTag::ChangedAfterFirstPick)
                )
            ),
        ]
    );
}

#[test]
fn an_earlier_question_holds_its_latest_version_and_what_the_agent_recorded() {
    let overview = RoundOverview::of(&saved("concluded"), None);

    let record = &overview.earlier[2];
    assert_eq!((record.number, record.question.id.as_str()), (3, "flush"));
    assert_eq!(record.question.version, 2);
    assert_eq!(
        record.answer,
        Some(kept(None, "A timer of five seconds, and on close.", None))
    );
    let interpretation = record.recorded.interpretation.as_ref().unwrap();
    assert_eq!(
        interpretation.recap,
        "Flush on a five second timer and on close"
    );
    assert_eq!(
        interpretation.follow_ups,
        vec!["Add the timer".to_owned(), "Flush on close".to_owned()]
    );
    assert_eq!(
        record.recorded.reply.as_deref(),
        Some("A timer bounds the loss in time.")
    );
    // One set of marks for the answer to each version: lines 4, then line 5.
    let marked: Vec<_> = record
        .marks
        .iter()
        .flat_map(|marks| &marks.reviewed)
        .map(|location| location.lines.as_ref().unwrap().first_line)
        .collect();
    assert_eq!(marked, vec![4, 5]);
}

#[test]
fn a_question_asked_again_after_another_is_a_step_of_its_own() {
    let overview = RoundOverview::of(&saved("reasked"), None);

    assert_eq!(
        overview.rail[1..4],
        [
            step(question(1), StepState::Done),
            step(question(2), StepState::Done),
            step(question(3), current(false)),
        ]
    );
    assert_eq!(overview.title, TabTitle::YourTurn { question: 3 });
    let first = &overview.earlier[0];
    assert_eq!(
        (first.question.id.as_str(), first.question.version),
        ("cache", 1)
    );
    // "None of the above" is the kept choice, with no tag.
    assert_eq!(
        first.answer,
        Some(kept(
            Some("None of the above"),
            "It depends on question two.",
            None
        ))
    );
    assert_eq!(numbers(&overview), (vec![1, 2], vec![1, 2]));
}

/// `legacy.json` was saved before rounds had a design, a quiz, first picks and review marks.
#[test]
fn a_round_saved_before_designs_and_quizzes_has_neither_step() {
    let overview = RoundOverview::of(&saved("legacy"), None);

    assert_eq!(
        overview.rail,
        vec![
            step(question(1), StepState::Done),
            step(Step::Conclusion, current(false)),
        ]
    );
    assert_eq!(overview.title, TabTitle::Conclusion);
    assert_eq!(
        overview.decisions,
        vec![Decision {
            number: 1,
            question: "Explain this line?".into(),
            answer: kept(Some("Keep it"), "Keep \"it\"\nwith a test", None),
        }]
    );
    let record = &overview.earlier[0];
    assert_eq!(
        record.recorded.interpretation.as_ref().unwrap().recap,
        "Recorded: keep"
    );
    assert!(record.marks.is_empty());
}

#[test]
fn the_overview_serializes_each_kind_by_name() {
    let overview = RoundOverview::of(&saved("working"), None);

    let value = serde_json::to_value(&overview).unwrap();
    assert_eq!(
        value["rail"][2],
        serde_json::json!({"step": {"kind": "question", "number": 2},
            "state": {"kind": "current", "working": false}})
    );
    assert_eq!(value["title"], serde_json::json!({"kind": "retry_needed"}));
    assert_eq!(
        value["decisions"][1]["answer"]["tag"],
        serde_json::json!("changed_after_first_pick")
    );
}

#[test]
fn the_marks_of_a_conclusion_go_with_the_answer_it_follows() {
    let mut round = saved("concluded");
    let conclusion = round.exploration.conclusion_request().unwrap().to_owned();
    let last = round.exploration.answers.last().unwrap().id.clone();
    let lines = review_source::SourceLineRange {
        first_line: 2,
        last_line: 3,
    };
    let location = CodeLocation {
        path: review_repository::repository::RepoPath::from_bytes(b"store.rs"),
        side: SourceSide::Old,
        lines: Some(lines),
    };
    round.marks.insert(
        conclusion,
        TurnMarks {
            answer: Some(last),
            reviewed: vec![location.clone()],
            ..TurnMarks::default()
        },
    );

    let overview = RoundOverview::of(&round, None);

    let marks = &overview.earlier[3].marks;
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0].reviewed, vec![location]);
}

#[test]
fn a_question_after_a_reply_to_the_conclusion_opens_a_step_before_the_quiz() {
    let overview = RoundOverview::of(&saved("replied"), None);

    assert_eq!(
        overview.rail[4..],
        [
            step(question(4), StepState::Done),
            step(question(5), current(false)),
            step(LATER_QUIZ, StepState::Later),
            step(Step::Conclusion, StepState::Later),
        ]
    );
    assert_eq!(overview.title, TabTitle::YourTurn { question: 5 });
    // The reply to the conclusion answers no question: it is no decision.
    assert_eq!(numbers(&overview), (vec![1, 2, 3, 4], vec![1, 2, 3, 4]));
}

/// The rail and the tab title of `overview`.
fn rail_and_title(overview: &RoundOverview) -> (Vec<RailStep>, TabTitle) {
    (overview.rail.clone(), overview.title)
}

#[test]
fn a_round_told_as_plain_facts_has_the_rail_and_the_title_of_its_saved_state() {
    let working = saved("working");
    let told = RoundStanding {
        design: true,
        questions: 2,
        activity: Activity::Working,
        latest: LatestTurn::Question,
    };
    assert_eq!(
        rail_and_title(&told.overview()),
        rail_and_title(&RoundOverview::of(&working, Some(&pending(&working))))
    );

    let quiz = saved("quiz");
    let conclusion = &quiz.exploration.conversation.last().unwrap().update;
    let told = RoundStanding {
        design: true,
        questions: 4,
        activity: Activity::Idle,
        latest: LatestTurn::Conclusion {
            quiz: Some(QuizProgress {
                items: 3,
                answers: quiz.exploration.quiz_answers(&conclusion.request),
            }),
        },
    };
    assert_eq!(
        rail_and_title(&told.overview()),
        rail_and_title(&RoundOverview::of(&quiz, None))
    );

    let kickoff = saved("kickoff");
    let told = RoundStanding {
        design: true,
        questions: 0,
        activity: Activity::Interrupted,
        latest: LatestTurn::None,
    };
    assert_eq!(
        rail_and_title(&told.overview()),
        rail_and_title(&RoundOverview::of(&kickoff, None))
    );
}
