//! Two front ends answering the same round: the session takes the first answer
//! and refuses the second, whatever copy of the round the second one holds.

use review_explore::Command;

use super::*;

impl Harness {
    /// Post `request` from a front end and return what the session answered.
    fn post(&mut self, request: &TurnRequest) -> ui_events::ExplorePosted {
        self.session
            .handle(Input::Command(Command::Turn(Box::new(request.clone()))));
        self.next::<ui_events::ExplorePosted>()
    }
}

#[test]
fn a_second_answer_to_a_question_the_agent_is_working_on_is_refused() {
    let mut harness = Harness::start();
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    assert!(applied(harness.submit(&access, question(&first, 1))));
    // The other front end's copy of the round, taken before the first answer.
    let mut stale = harness.exploration().clone();
    let (answered, renewed) = harness.answer("Keep it.");

    let shown = stale.questions.last().cloned();
    let second = stale
        .request(
            Some(AnswerInput {
                option: Some("change".into()),
                text: "Change it.".into(),
                ..AnswerInput::default()
            }),
            shown.as_ref(),
        )
        .unwrap();
    let refused = harness.post(&second);

    assert!(refused.result.is_err(), "{:?}", refused.result);
    let saved = harness.saved();
    assert_eq!(
        saved.exploration.answers,
        vec![answered.answer.clone().unwrap()]
    );
    assert_eq!(
        saved.exploration.pending_request().map(|r| &r.request),
        Some(&answered.request)
    );
    assert_eq!(
        harness.agents.prompts().len(),
        2,
        "no prompt for the refusal"
    );
    // The agent still answers the first answer with the access it was given.
    assert!(applied(harness.submit(&renewed, question(&answered, 2))));
    assert_eq!(harness.saved().exploration.questions.len(), 2);
}

#[test]
fn a_choice_on_a_question_answered_elsewhere_is_refused_once_the_agent_moved_on() {
    let mut harness = Harness::start();
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    assert!(applied(harness.submit(&access, question(&first, 1))));
    let mut stale = harness.exploration().clone();
    let (answered, renewed) = harness.answer("Keep it.");
    assert!(applied(harness.submit(&renewed, question(&answered, 2))));

    let first_question = stale.questions.first().cloned();
    let late = stale
        .request(
            Some(AnswerInput {
                option: Some("change".into()),
                text: String::new(),
                ..AnswerInput::default()
            }),
            first_question.as_ref(),
        )
        .unwrap();
    let refused = harness.post(&late);

    assert!(refused.result.is_err(), "{:?}", refused.result);
    let saved = harness.saved();
    assert_eq!(saved.exploration.answers.len(), 1);
    assert_eq!(saved.exploration.pending_request(), None);
}

#[test]
fn a_refused_second_answer_leaves_the_first_answers_prompt_delivered() {
    let mut harness = Harness::start();
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    assert!(applied(harness.submit(&access, question(&first, 1))));
    let mut stale = harness.exploration().clone();
    // The first answer's prompt stays queued while the second answer arrives.
    harness.delivery.close();
    let answered = harness.request(Some(AnswerInput {
        option: Some("keep".into()),
        text: "Keep it.".into(),
        ..AnswerInput::default()
    }));
    assert!(harness.post(&answered).result.is_ok());

    let shown = stale.questions.last().cloned();
    let second = stale
        .request(
            Some(AnswerInput {
                option: Some("change".into()),
                text: "Change it.".into(),
                ..AnswerInput::default()
            }),
            shown.as_ref(),
        )
        .unwrap();
    assert!(harness.post(&second).result.is_err());
    harness.delivery.open();

    let prompt = harness.delivered_prompt();
    let answer = answered.answer.unwrap();
    assert!(prompt.contains(&format!("Answer ID: {}\n", answer.id)));
    assert_eq!(harness.saved().exploration.answers, vec![answer]);
}
