use super::*;
use review_explore_round_settings::WritingStyle;

#[test]
fn every_request_of_a_round_carries_the_writing_style_it_started_with_after_a_restore_too() {
    let mut exploration = exploration();
    exploration.writing = WritingStyle::SimplifiedTechnicalEnglish;
    let kickoff = exploration.request(None, None).unwrap();
    assert_eq!(kickoff.writing, WritingStyle::SimplifiedTechnicalEnglish);
    let mut response = update(&kickoff, Some(question(1)));
    response.topics.push(policy_topic());
    assert!(exploration.apply(response).unwrap());

    let saved = serde_json::to_value(ExploreRound::new(exploration)).unwrap();
    let mut restored: ExploreRound = serde_json::from_value(saved).unwrap();
    let question = restored.exploration.questions[0].clone();
    let answer = restored
        .exploration
        .request(
            Some(AnswerInput {
                option: Some("keep".into()),
                ..AnswerInput::default()
            }),
            Some(&question),
        )
        .unwrap();

    assert_eq!(
        restored.exploration.writing,
        WritingStyle::SimplifiedTechnicalEnglish
    );
    assert_eq!(answer.writing, WritingStyle::SimplifiedTechnicalEnglish);
}

#[test]
fn a_round_saved_before_writing_styles_existed_keeps_the_agents_own_style() {
    let mut exploration = exploration();
    let request = exploration.request(None, None).unwrap();
    let mut saved = serde_json::to_value(ExploreRound::new(exploration)).unwrap();
    saved["exploration"]
        .as_object_mut()
        .unwrap()
        .remove("writing");
    let mut request = serde_json::to_value(request).unwrap();
    request.as_object_mut().unwrap().remove("writing");

    let round: ExploreRound = serde_json::from_value(saved).unwrap();
    let request: TurnRequest = serde_json::from_value(request).unwrap();

    assert_eq!(round.exploration.writing, WritingStyle::Plain);
    assert_eq!(request.writing, WritingStyle::Plain);
}
