use super::*;

const BROKEN: &str = "flowchart LR\n  A --> B[label (x)]";

fn broken_diagram(version: u32) -> DiagramError {
    DiagramError {
        question: "q".into(),
        version,
        source: BROKEN.into(),
        message: "Parse error on line 2: got 'PS'".into(),
    }
}

#[test]
fn a_diagram_error_is_kept_with_its_question_and_saved() {
    let mut exploration = started();
    assert!(exploration.record_diagram_error(broken_diagram(1)).unwrap());
    assert_eq!(exploration.diagram_errors, vec![broken_diagram(1)]);

    let saved: Exploration =
        serde_json::from_value(serde_json::to_value(&exploration).unwrap()).unwrap();
    assert_eq!(saved.diagram_errors, vec![broken_diagram(1)]);
}

#[test]
fn the_same_diagram_error_again_changes_nothing() {
    let mut exploration = started();
    exploration.record_diagram_error(broken_diagram(1)).unwrap();
    assert!(!exploration.record_diagram_error(broken_diagram(1)).unwrap());
    assert_eq!(exploration.diagram_errors.len(), 1);
}

#[test]
fn a_new_message_for_the_same_diagram_replaces_the_old_one() {
    let mut exploration = started();
    exploration.record_diagram_error(broken_diagram(1)).unwrap();
    let again = DiagramError {
        message: "Lexical error on line 2".into(),
        ..broken_diagram(1)
    };
    assert!(exploration.record_diagram_error(again.clone()).unwrap());
    assert_eq!(exploration.diagram_errors, vec![again]);
}

#[test]
fn a_diagram_error_for_a_question_the_round_never_posted_is_refused() {
    let mut exploration = started();
    assert!(exploration.record_diagram_error(broken_diagram(2)).is_err());
    let unknown = DiagramError {
        question: "other".into(),
        ..broken_diagram(1)
    };
    assert!(exploration.record_diagram_error(unknown).is_err());
    assert!(exploration.diagram_errors.is_empty());
}

#[test]
fn a_round_saved_before_diagram_errors_existed_still_loads() {
    let mut saved = serde_json::to_value(started()).unwrap();
    saved.as_object_mut().unwrap().remove("diagram_errors");
    let exploration: Exploration = serde_json::from_value(saved).unwrap();
    assert!(exploration.diagram_errors.is_empty());
}

#[test]
fn cancelling_the_answer_a_question_followed_drops_its_diagram_errors() {
    let mut exploration = started();
    let first = exploration.questions[0].clone();
    let request = exploration
        .request(
            Some(AnswerInput {
                text: "What does reopening cost?".into(),
                ..AnswerInput::default()
            }),
            Some(&first),
        )
        .unwrap();
    let answer = request.answer.as_ref().unwrap().id.clone();
    let mut response = update(&request, Some(question(2)));
    response.interpretation = Some(Interpretation {
        answer: answer.clone(),
        status: TopicStatus::Open,
        recap: "Recorded: reopen, details pending".into(),
        follow_ups: vec![],
    });
    assert!(exploration.apply(response).unwrap());
    exploration.record_diagram_error(broken_diagram(1)).unwrap();
    exploration.record_diagram_error(broken_diagram(2)).unwrap();

    let mut round = ExploreRound::new(exploration);
    round.cancel_answer(&answer).unwrap();
    assert_eq!(round.exploration.diagram_errors, vec![broken_diagram(1)]);
}
