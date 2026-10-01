use super::*;

/// A pass whose first question was answered with "keep", the agent's
/// concluding turn after it accepted, and its marks recorded.
fn concluded() -> (ExplorePass, TurnRequest) {
    let mut pass = opened();
    let request = answer(&pass, "keep");
    pass.post(&request).unwrap();
    let mut response = update(&request, None);
    response.interpretation = Some(Interpretation {
        answer: request.answer.as_ref().unwrap().id.clone(),
        status: TopicStatus::Accepted,
        recap: "Recorded: keep resolved".into(),
        follow_ups: vec![],
    });
    response.findings.push("Keep resolved conversations".into());
    assert!(pass.submit(&response).unwrap());
    pass.marks.insert(
        request.request.clone(),
        TurnMarks {
            answer: request.answer.as_ref().unwrap().id.clone(),
            ..TurnMarks::default()
        },
    );
    pass.completion = Some(ReviewCompletion {
        request: request.request.clone(),
        baseline: "checkpoint".into(),
    });
    (pass, request)
}

/// A pass whose agent asked its first question.
fn opened() -> ExplorePass {
    let mut pass = ExplorePass::new(exploration());
    let kickoff = pass.exploration.clone().request(None, None).unwrap();
    pass.post(&kickoff).unwrap();
    let mut opening = update(&kickoff, Some(question(1)));
    opening.topics = started().topics.into_values().collect();
    assert!(pass.submit(&opening).unwrap());
    pass
}

/// The reviewer's request choosing `option` on the latest question.
fn answer(pass: &ExplorePass, option: &str) -> TurnRequest {
    let mut copy = pass.exploration.clone();
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
    let (mut pass, request) = concluded();
    let id = request.answer.as_ref().unwrap().id.clone();
    let opening = pass.exploration.conversation[0].clone();

    let cancelled = pass.cancel_answer(&id).unwrap();

    assert_eq!(cancelled.answer, *request.answer.as_ref().unwrap());
    assert_eq!(cancelled.request.as_deref(), Some(request.request.as_str()));
    assert_eq!(cancelled.marks.unwrap().answer, id);
    let exploration = &pass.exploration;
    assert_eq!(exploration.conversation, vec![opening]);
    assert!(exploration.answers.is_empty() && exploration.interpretations.is_empty());
    assert!(exploration.findings.is_empty() && exploration.conclusion.is_none());
    assert_eq!(exploration.topics["policy"].status, TopicStatus::Open);
    assert!(exploration.can_choose(&exploration.questions[0]));
    assert!(pass.turns.len() == 1 && pass.marks.is_empty() && pass.completion.is_none());
    pass.validate_restored().unwrap();
}

#[test]
fn the_next_request_names_every_cancelled_answer_once() {
    let (mut pass, request) = concluded();
    let first = request.answer.as_ref().unwrap().id.clone();
    pass.cancel_answer(&first).unwrap();
    let again = answer(&pass, "change");
    pass.post(&again).unwrap();
    let second = again.answer.as_ref().unwrap().id.clone();
    // Cancelled before the agent answered: the pending request is withdrawn.
    pass.cancel_answer(&second).unwrap();
    assert!(pass.exploration.pending_request().is_none());
    let stale = update(&again, None);
    assert!(pass.submit(&stale).is_err());

    let next = answer(&pass, "keep");
    assert_eq!(next.cancelled, vec![first, second]);
    let mut forgetful = next.clone();
    forgetful.cancelled.clear();
    assert!(pass.clone().post(&forgetful).is_err());
    pass.post(&next).unwrap();
    assert_eq!(pass.exploration.pending_request(), Some(&next));
}

#[test]
fn only_the_latest_answer_can_be_cancelled_and_not_after_implementation() {
    let (mut pass, request) = concluded();
    let id = request.answer.as_ref().unwrap().id.clone();
    assert!(pass.cancel_answer("another").is_err());
    let implementation = pass
        .exploration
        .implementation("Add a test".into())
        .unwrap();
    assert!(pass.authorize(&implementation).unwrap());
    let error = pass.cancel_answer(&id).unwrap_err().to_string();
    assert!(error.contains("Implementation was requested"), "{error}");
    assert_eq!(pass.exploration.answers.len(), 1);
}

#[test]
fn an_interrupted_delivery_of_a_cancelled_answer_is_forgotten_too() {
    let mut pass = opened();
    let first = answer(&pass, "keep");
    pass.post(&first).unwrap();
    pass.exploration.pause_delivery();
    let mut copy = pass.exploration.clone();
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
    pass.post(&second).unwrap();

    pass.cancel_answer(&second.answer.as_ref().unwrap().id)
        .unwrap();
    pass.cancel_answer(&first.answer.as_ref().unwrap().id)
        .unwrap();

    assert_eq!(pass.turns.len(), 1);
    pass.validate_restored().unwrap();
    let next = answer(&pass, "keep");
    assert_eq!(
        next.cancelled,
        vec![second.answer.unwrap().id, first.answer.unwrap().id]
    );
}
