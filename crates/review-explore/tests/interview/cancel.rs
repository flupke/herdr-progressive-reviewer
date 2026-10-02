use super::*;

/// A round whose first question was answered with "keep", the agent's
/// concluding turn after it accepted, and its marks recorded.
fn concluded() -> (ExploreRound, TurnRequest) {
    let mut round = opened();
    let request = answer(&round, "keep");
    round.post(&request).unwrap();
    let mut response = update(&request, None);
    response.interpretation = Some(Interpretation {
        answer: request.answer.as_ref().unwrap().id.clone(),
        status: TopicStatus::Accepted,
        recap: "Recorded: keep resolved".into(),
        follow_ups: vec![],
    });
    response.findings.push("Keep resolved conversations".into());
    assert!(round.submit(&response).unwrap());
    round.marks.insert(
        request.request.clone(),
        TurnMarks {
            answer: request.answer.as_ref().unwrap().id.clone(),
            ..TurnMarks::default()
        },
    );
    round.completion = Some(ReviewCompletion {
        request: request.request.clone(),
        baseline: "checkpoint".into(),
    });
    (round, request)
}

/// A round whose agent asked its first question.
fn opened() -> ExploreRound {
    let mut round = ExploreRound::new(exploration());
    let kickoff = round.exploration.clone().request(None, None).unwrap();
    round.post(&kickoff).unwrap();
    let mut opening = update(&kickoff, Some(question(1)));
    opening.topics = started().topics.into_values().collect();
    assert!(round.submit(&opening).unwrap());
    round
}

/// The reviewer's request choosing `option` on the latest question.
fn answer(round: &ExploreRound, option: &str) -> TurnRequest {
    let mut copy = round.exploration.clone();
    let question = copy.questions.last().cloned().unwrap();
    copy.request(
        Some(AnswerInput {
            option: Some(option.into()),
            ..AnswerInput::default()
        }),
        Some(&question),
    )
    .unwrap()
}

#[test]
fn cancelling_the_latest_answer_rewinds_to_its_question() {
    let (mut round, request) = concluded();
    let id = request.answer.as_ref().unwrap().id.clone();
    let opening = round.exploration.conversation[0].clone();

    let cancelled = round.cancel_answer(&id).unwrap();

    assert_eq!(cancelled.answer, *request.answer.as_ref().unwrap());
    assert_eq!(cancelled.request.as_deref(), Some(request.request.as_str()));
    assert_eq!(cancelled.marks.unwrap().answer, id);
    let exploration = &round.exploration;
    assert_eq!(exploration.conversation, vec![opening]);
    assert!(exploration.answers.is_empty() && exploration.interpretations.is_empty());
    assert!(exploration.findings.is_empty() && exploration.conclusion.is_none());
    assert_eq!(exploration.topics["policy"].status, TopicStatus::Open);
    assert!(exploration.can_choose(&exploration.questions[0]));
    assert!(round.turns.len() == 1 && round.marks.is_empty() && round.completion.is_none());
    round.validate_restored().unwrap();
}

#[test]
fn the_next_request_names_every_cancelled_answer_once() {
    let (mut round, request) = concluded();
    let first = request.answer.as_ref().unwrap().id.clone();
    round.cancel_answer(&first).unwrap();
    let again = answer(&round, "change");
    round.post(&again).unwrap();
    let second = again.answer.as_ref().unwrap().id.clone();
    // Cancelled before the agent answered: the pending request is withdrawn.
    round.cancel_answer(&second).unwrap();
    assert!(round.exploration.pending_request().is_none());
    let stale = update(&again, None);
    assert!(round.submit(&stale).is_err());

    let next = answer(&round, "keep");
    assert_eq!(next.cancelled, vec![first, second]);
    let mut forgetful = next.clone();
    forgetful.cancelled.clear();
    assert!(round.clone().post(&forgetful).is_err());
    round.post(&next).unwrap();
    assert_eq!(round.exploration.pending_request(), Some(&next));
}

#[test]
fn only_the_latest_answer_can_be_cancelled_and_not_after_implementation() {
    let (mut round, request) = concluded();
    let id = request.answer.as_ref().unwrap().id.clone();
    assert!(round.cancel_answer("another").is_err());
    let implementation = round
        .exploration
        .implementation("Add a test".into())
        .unwrap();
    assert!(round.authorize(&implementation).unwrap());
    let error = round.cancel_answer(&id).unwrap_err().to_string();
    assert!(error.contains("Implementation was requested"), "{error}");
    assert_eq!(round.exploration.answers.len(), 1);
}

#[test]
fn an_interrupted_delivery_of_a_cancelled_answer_is_forgotten_too() {
    let mut round = opened();
    let first = answer(&round, "keep");
    round.post(&first).unwrap();
    round.exploration.pause_delivery();
    let mut copy = round.exploration.clone();
    let question = copy.questions[0].clone();
    let second = copy
        .request(
            Some(AnswerInput {
                text: "Also check the caller.".into(),
                ..AnswerInput::default()
            }),
            Some(&question),
        )
        .unwrap();
    round.post(&second).unwrap();

    round
        .cancel_answer(&second.answer.as_ref().unwrap().id)
        .unwrap();
    round
        .cancel_answer(&first.answer.as_ref().unwrap().id)
        .unwrap();

    assert_eq!(round.turns.len(), 1);
    round.validate_restored().unwrap();
    let next = answer(&round, "keep");
    assert_eq!(
        next.cancelled,
        vec![second.answer.unwrap().id, first.answer.unwrap().id]
    );
}
