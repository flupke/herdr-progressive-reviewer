use super::*;

/// An item whose correct option is the second, proven by the first two lines of the base.
fn item(question: &str) -> QuizItem {
    QuizItem {
        question: question.into(),
        answers: vec![
            "Reopened on every change".into(),
            "Kept resolved until a new comment".into(),
            "Deleted with the change".into(),
        ],
        correct: 1,
        why: "The policy reopens a conversation only for a new comment.".into(),
        proof: question_evidence(),
        level: "Data model: the states a conversation goes through.".into(),
    }
}

fn question_evidence() -> Vec<EvidenceRef> {
    question(1).evidence
}

/// Concludes a new round at once, with `quiz`.
fn concluded(quiz: Vec<QuizItem>) -> eyre::Result<(Exploration, String)> {
    let mut exploration = exploration();
    let request = exploration.request(None, None).unwrap();
    let mut response = update(&request, None);
    response.conclusion = Some(Conclusion {
        summary: "Resolved conversations stay resolved.".into(),
        quiz,
        ..Conclusion::default()
    });
    exploration.apply(response)?;
    Ok((exploration, request.request))
}

fn refusal(quiz: Vec<QuizItem>) -> String {
    concluded(quiz).unwrap_err().to_string()
}

#[test]
fn a_conclusion_carries_its_quiz_into_the_saved_round() {
    let (exploration, _) =
        concluded(vec![item("A conversation was resolved. What then?")]).unwrap();

    let saved: Exploration =
        serde_json::from_value(serde_json::to_value(&exploration).unwrap()).unwrap();
    let quiz = &saved.conclusion.as_ref().unwrap().quiz;
    assert_eq!(quiz, &vec![item("A conversation was resolved. What then?")]);
}

#[test]
fn an_item_without_its_correct_answer_is_refused() {
    let wrong = QuizItem {
        correct: 3,
        ..item("A conversation was resolved. What then?")
    };
    assert!(refusal(vec![wrong]).contains("correct"));
}

#[test]
fn an_item_without_proof_lines_is_refused() {
    let none = QuizItem {
        proof: vec![],
        ..item("A conversation was resolved. What then?")
    };
    let mut whole_file = item("A conversation was resolved. What then?");
    whole_file.proof[0].location.lines = None;
    let mut elsewhere = item("A conversation was resolved. What then?");
    elsewhere.proof[0].location.lines = Some(SourceLineRange {
        first_line: 7,
        last_line: 9,
    });

    for item in [none, whole_file, elsewhere] {
        assert!(refusal(vec![item]).contains("proof"));
    }
}

#[test]
fn an_item_needs_two_to_four_distinct_answers() {
    let one = QuizItem {
        answers: vec!["Kept".into()],
        correct: 0,
        ..item("A conversation was resolved. What then?")
    };
    let twice = QuizItem {
        answers: vec!["Kept".into(), "kept ".into()],
        correct: 0,
        ..item("A conversation was resolved. What then?")
    };
    for item in [one, twice] {
        assert!(refusal(vec![item]).contains("answers"));
    }
}

#[test]
fn a_quiz_holds_at_most_three_items() {
    let quiz = ["one", "two", "three", "four"].map(item).to_vec();
    assert!(refusal(quiz).contains("at most 3"));
    assert!(concluded(["one", "two", "three"].map(item).to_vec()).is_ok());
}

#[test]
fn an_empty_quiz_says_why_and_a_quiz_with_items_does_not() {
    assert!(refusal(vec![]).contains("quiz_empty_reason"));

    let mut exploration = exploration();
    let request = exploration.request(None, None).unwrap();
    let mut response = update(&request, None);
    let conclusion = response.conclusion.as_mut().unwrap();
    conclusion.quiz = vec![item("A conversation was resolved. What then?")];
    conclusion.quiz_empty_reason = Some("Nothing at whiteboard level.".into());
    let error = exploration.apply(response).unwrap_err().to_string();
    assert!(error.contains("quiz_empty_reason"), "{error}");
}

#[test]
fn the_reviewer_answers_the_quiz_in_order_and_each_pick_is_graded() {
    let (mut exploration, conclusion) = concluded(vec![item("First?"), item("Second?")]).unwrap();

    let out_of_order = QuizResponse::Pick { item: 1, answer: 1 };
    assert!(exploration.answer_quiz(&conclusion, out_of_order).is_err());
    let right = QuizResponse::Pick { item: 0, answer: 1 };
    assert!(exploration.answer_quiz(&conclusion, right).unwrap());
    assert!(
        !exploration.answer_quiz(&conclusion, right).unwrap(),
        "the same pick again"
    );
    let changed = QuizResponse::Pick { item: 0, answer: 2 };
    assert!(exploration.answer_quiz(&conclusion, changed).is_err());
    let wrong = QuizResponse::Pick { item: 1, answer: 0 };
    assert!(exploration.answer_quiz(&conclusion, wrong).unwrap());

    let answers = exploration.quiz_answers(&conclusion).unwrap();
    assert_eq!(
        answers.picks,
        vec![
            QuizPick {
                item: 0,
                answer: 1,
                correct: true
            },
            QuizPick {
                item: 1,
                answer: 0,
                correct: false
            },
        ]
    );
    assert_eq!(answers.next_item(2), None);
    assert_eq!(answers.correct_picks(), 1);
}

#[test]
fn a_pick_of_an_item_or_option_out_of_range_is_refused() {
    let (mut exploration, conclusion) = concluded(vec![item("First?")]).unwrap();

    for forged in [
        QuizResponse::Pick {
            item: usize::MAX,
            answer: 0,
        },
        QuizResponse::Pick {
            item: 0,
            answer: usize::MAX,
        },
    ] {
        assert!(exploration.answer_quiz(&conclusion, forged).is_err());
    }
    assert_eq!(
        exploration
            .quiz_answers(&conclusion)
            .map_or(0, |answers| answers.picks.len()),
        0
    );
}

#[test]
fn a_skipped_quiz_takes_no_more_picks_and_keeps_the_earlier_ones() {
    let (mut exploration, conclusion) = concluded(vec![item("First?"), item("Second?")]).unwrap();
    let first = QuizResponse::Pick { item: 0, answer: 0 };
    exploration.answer_quiz(&conclusion, first).unwrap();

    assert!(
        exploration
            .answer_quiz(&conclusion, QuizResponse::Skip)
            .unwrap()
    );
    assert!(
        !exploration
            .answer_quiz(&conclusion, QuizResponse::Skip)
            .unwrap()
    );
    let second = QuizResponse::Pick { item: 1, answer: 1 };
    assert!(exploration.answer_quiz(&conclusion, second).is_err());

    let answers = exploration.quiz_answers(&conclusion).unwrap();
    assert!(answers.skipped);
    assert_eq!(
        answers.picks,
        vec![QuizPick {
            item: 0,
            answer: 0,
            correct: false
        }]
    );
    assert_eq!(answers.next_item(2), None);
}

#[test]
fn the_quiz_answers_are_saved_with_the_round() {
    let (mut exploration, conclusion) = concluded(vec![item("First?")]).unwrap();
    let pick = QuizResponse::Pick { item: 0, answer: 1 };
    exploration.answer_quiz(&conclusion, pick).unwrap();

    let saved: Exploration =
        serde_json::from_value(serde_json::to_value(&exploration).unwrap()).unwrap();
    assert_eq!(
        saved.quiz_answers(&conclusion),
        exploration.quiz_answers(&conclusion)
    );
}

#[test]
fn only_the_quiz_of_the_current_conclusion_takes_answers() {
    let (mut exploration, conclusion) = concluded(vec![item("First?")]).unwrap();
    let pick = QuizResponse::Pick { item: 0, answer: 1 };
    assert!(exploration.answer_quiz("another-turn", pick).is_err());
    assert!(exploration.answer_quiz(&conclusion, pick).unwrap());

    let mut without_quiz = exploration_without_quiz();
    let request = without_quiz.conversation[0].update.request.clone();
    assert!(without_quiz.answer_quiz(&request, pick).is_err());
}

/// A round concluded at once with an empty quiz.
fn exploration_without_quiz() -> Exploration {
    let mut exploration = exploration();
    let request = exploration.request(None, None).unwrap();
    assert!(exploration.apply(update(&request, None)).unwrap());
    exploration
}

#[test]
fn a_round_saved_before_quizzes_existed_still_loads() {
    let (exploration, conclusion) = concluded(vec![item("First?")]).unwrap();
    let mut saved = serde_json::to_value(&exploration).unwrap();
    for turn in saved["conversation"].as_array_mut().unwrap() {
        let stored = turn["update"]["conclusion"].as_object_mut().unwrap();
        stored.remove("quiz");
        stored.remove("quiz_empty_reason");
    }
    let stored = saved["conclusion"].as_object_mut().unwrap();
    stored.remove("quiz");
    saved.as_object_mut().unwrap().remove("quiz_answers");

    let loaded: Exploration = serde_json::from_value(saved).unwrap();
    assert!(loaded.conclusion.as_ref().unwrap().quiz.is_empty());
    assert_eq!(loaded.quiz_answers(&conclusion), None);
}

#[test]
fn cancelling_the_answer_a_conclusion_followed_drops_its_quiz_answers() {
    let mut exploration = started();
    let question = exploration.questions[0].clone();
    let request = exploration
        .request(
            Some(AnswerInput {
                text: "What does reopening cost?".into(),
                ..AnswerInput::default()
            }),
            Some(&question),
        )
        .unwrap();
    let answer = request.answer.as_ref().unwrap().id.clone();
    let mut response = update(&request, None);
    response.conclusion.as_mut().unwrap().quiz = vec![item("First?")];
    response.conclusion.as_mut().unwrap().quiz_empty_reason = None;
    assert!(exploration.apply(response).unwrap());
    let pick = QuizResponse::Pick { item: 0, answer: 1 };
    exploration.answer_quiz(&request.request, pick).unwrap();

    let mut round = ExploreRound::new(exploration);
    round.cancel_answer(&answer).unwrap();
    assert_eq!(round.exploration.quiz_answers(&request.request), None);
}
