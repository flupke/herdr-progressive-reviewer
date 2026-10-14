use super::*;

/// A round whose reviewer sent the agent back to the drawing board on its first question, and
/// the request that took them there.
fn at_drawing_board() -> (Exploration, TurnRequest) {
    let mut exploration = started();
    let question = exploration.questions[0].clone();
    let request = exploration
        .request(
            Some(AnswerInput {
                drawing_board: true,
                ..AnswerInput::default()
            }),
            Some(&question),
        )
        .unwrap();
    (exploration, request)
}

/// The agent's conclusion at the drawing board, with the changes the design needs.
fn changes(request: &TurnRequest) -> InterviewUpdate {
    let mut response = update(request, None);
    response.conclusion = Some(Conclusion {
        to_be_implemented: "Keep one queue per conversation.".into(),
        ..conclusion("The reviewer wants one queue per conversation.")
    });
    response
}

#[test]
fn the_drawing_board_answers_the_open_question_with_no_comment_and_no_choice() {
    // An answer with neither is refused, unless it sends the agent back to the drawing board.
    let (exploration, request) = at_drawing_board();
    let answer = request.answer.unwrap();

    assert!(answer.drawing_board);
    assert_eq!(answer.question.as_ref(), Some(&exploration.questions[0]));
}

#[test]
fn the_drawing_board_answers_a_question_and_picks_no_choice() {
    let mut exploration = started();
    let question = exploration.questions[0].clone();
    let with_a_choice = AnswerInput {
        drawing_board: true,
        option: Some("keep".into()),
        ..AnswerInput::default()
    };

    let chooses = exploration
        .request(Some(with_a_choice), Some(&question))
        .unwrap_err()
        .to_string();
    assert!(chooses.contains("with no choice"), "{chooses}");
    let (mut exploration, request) = at_drawing_board();
    assert!(exploration.apply(changes(&request)).unwrap());
    let after_the_conclusion = AnswerInput {
        drawing_board: true,
        ..AnswerInput::default()
    };
    let replies = exploration
        .request(Some(after_the_conclusion), None)
        .unwrap_err()
        .to_string();
    assert!(
        replies.contains("takes the place of an answer"),
        "{replies}"
    );
}

#[test]
fn the_agent_at_the_drawing_board_concludes_with_the_changes_and_nothing_else() {
    let (exploration, request) = at_drawing_board();
    let refused = |change: &dyn Fn(&mut InterviewUpdate)| {
        let mut response = changes(&request);
        change(&mut response);
        exploration.clone().apply(response).unwrap_err().to_string()
    };

    let asks = refused(&|response| {
        response.conclusion = None;
        response.next = Some(question(2));
    });
    assert!(asks.contains("conclude with submit_conclusion"), "{asks}");
    let no_changes = refused(&|response| {
        response.conclusion.as_mut().unwrap().to_be_implemented = " ".into();
    });
    assert!(no_changes.contains("to_be_implemented"), "{no_changes}");
    let quiz = refused(&|response| {
        let conclusion = response.conclusion.as_mut().unwrap();
        conclusion.quiz = vec![quiz::item("Where does a resolved conversation go?")];
        conclusion.quiz_empty_reason = None;
    });
    assert!(quiz.contains("leave quiz empty"), "{quiz}");
    let decides = refused(&|response| {
        response.interpretation = Some(Interpretation {
            answer: request.answer.as_ref().unwrap().id.clone(),
            status: TopicStatus::NeedsFollowUp,
            recap: "Recorded: one queue per conversation.".into(),
            follow_ups: vec![],
        });
    });
    assert!(decides.contains("leave interpretation null"), "{decides}");
    let marks = refused(&|response| response.reviewed = vec![policy_lines(1, 1)]);
    assert!(marks.contains("The code will change"), "{marks}");
    let mut exploration = exploration;
    assert!(exploration.apply(changes(&request)).unwrap());
    assert_eq!(
        exploration.conclusion.clone().unwrap().to_be_implemented,
        "Keep one queue per conversation."
    );
}

#[test]
fn the_reviewer_s_decisions_show_the_drawing_board_in_place_of_a_choice() {
    let (mut exploration, request) = at_drawing_board();
    assert!(exploration.apply(changes(&request)).unwrap());

    let decisions = Decision::of_round(&exploration);

    let answer = &decisions.last().unwrap().answer;
    assert!(answer.drawing_board);
    assert_eq!(answer.choice.as_deref(), Some("Back to the drawing board"));
}

#[test]
fn a_reply_to_the_conclusion_at_the_drawing_board_stays_there() {
    let (mut exploration, request) = at_drawing_board();
    assert!(exploration.apply(changes(&request)).unwrap());
    let reply = AnswerInput {
        text: "Keep the cache.".into(),
        ..AnswerInput::default()
    };

    let request = exploration.request(Some(reply), None).unwrap();

    assert!(request.at_drawing_board());
    let mut asks = update(&request, Some(question(2)));
    asks.conclusion = None;
    let refused = exploration.clone().apply(asks).unwrap_err().to_string();
    assert!(
        refused.contains("conclude with submit_conclusion"),
        "{refused}"
    );
    assert!(exploration.apply(changes(&request)).unwrap());
}

#[test]
fn a_round_concluded_at_the_drawing_board_says_so_and_another_does_not() {
    let (mut exploration, request) = at_drawing_board();
    assert!(
        !exploration.concluded_at_drawing_board(),
        "no conclusion yet"
    );
    assert!(exploration.apply(changes(&request)).unwrap());
    let mut commented = started();
    let question = commented.questions[0].clone();
    let comment = AnswerInput {
        text: "Why one queue?".into(),
        ..AnswerInput::default()
    };
    let request = commented.request(Some(comment), Some(&question)).unwrap();
    let mut concludes = update(&request, None);
    concludes.conclusion = Some(conclusion("One queue keeps the replies in order."));
    assert!(commented.apply(concludes).unwrap());

    assert!(exploration.concluded_at_drawing_board());
    assert!(!commented.concluded_at_drawing_board());
}
