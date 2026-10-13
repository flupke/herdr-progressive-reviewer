use super::*;

/// The agent's version `version` of the round's open question, after the reviewer's message.
fn revision(exploration: &Exploration, version: u32) -> QuestionRevision {
    QuestionRevision {
        instance: exploration.instance.clone(),
        checkpoint: exploration.comparison.checkpoint.clone(),
        revision: Revision {
            message: "message-1".into(),
            reason: "You said that readers rebuild the cache themselves.".into(),
        },
        question: Question {
            text: format!("Keep resolved conversations resolved, as version {version} asks?"),
            ..question(version)
        },
        glossary: std::collections::BTreeMap::new(),
    }
}

fn first_line() -> CodeLocation {
    CodeLocation {
        path: review_repository::repository::RepoPath::from_bytes(b"policy.rs"),
        side: SourceSide::Old,
        lines: Some(SourceLineRange {
            first_line: 1,
            last_line: 1,
        }),
    }
}

/// A round whose reviewer answered its first question with a comment, and whose agent asked
/// version 2 of it, asking to mark the first line of policy.rs once the reviewer answers.
fn asked_again() -> (Exploration, ReviewerAnswer) {
    let mut exploration = started();
    let question = exploration.questions[0].clone();
    let request = exploration
        .request(
            Some(AnswerInput {
                text: "Readers rebuild it.".into(),
                ..AnswerInput::default()
            }),
            Some(&question),
        )
        .unwrap();
    let mut response = update(&request, Some(self::question(2)));
    response.reviewed = vec![first_line()];
    assert!(exploration.apply(response).unwrap());
    (exploration, request.answer.unwrap())
}

#[test]
fn a_revision_replaces_the_open_question_and_its_answer_applies_the_marks_it_waited_for() {
    let (mut exploration, answer) = asked_again();
    let asked = exploration.conversation.last().unwrap().clone();

    assert!(exploration.revise(revision(&exploration, 3)).unwrap());

    let revised = exploration.conversation.last().unwrap();
    assert_eq!(revised.answer.as_ref(), Some(&answer.id));
    assert_eq!(revised.revision, Some(revision(&exploration, 3).revision));
    assert_ne!(revised.update.request, asked.update.request);
    assert_eq!(revised.update.reviewed, vec![first_line()]);
    assert_eq!(revised.update.reply, asked.update.reply);
    let open = exploration.waiting_turn().unwrap().next.clone().unwrap();
    assert_eq!(open, revision(&exploration, 3).question);
    assert_eq!(exploration.question_number(&open), Some(1));
    let request = exploration
        .request(
            Some(AnswerInput {
                option: Some("keep".into()),
                ..AnswerInput::default()
            }),
            Some(&open),
        )
        .unwrap();
    assert_eq!(
        request.answer.unwrap().in_reply_to,
        exploration.conversation.last().unwrap().update.request
    );
}

#[test]
fn a_retried_revision_changes_nothing_and_a_different_one_of_the_same_version_is_refused() {
    let (mut exploration, _) = asked_again();
    assert!(exploration.revise(revision(&exploration, 3)).unwrap());
    let before = exploration.clone();

    assert!(!exploration.revise(revision(&exploration, 3)).unwrap());
    let mut other = revision(&exploration, 3);
    other.question.text = "Another question".into();
    assert!(exploration.revise(other).is_err());
    assert_eq!(exploration, before);
}

#[test]
fn only_a_higher_version_of_the_open_question_is_a_revision() {
    let (mut exploration, _) = asked_again();
    let before = exploration.clone();

    let error = exploration
        .revise(revision(&exploration, 2))
        .unwrap_err()
        .to_string();
    assert!(error.contains("open question q"), "{error}");
    let mut other = revision(&exploration, 3);
    other.question.id = "other".into();
    assert!(exploration.revise(other).is_err());
    let mut silent = revision(&exploration, 3);
    silent.revision.reason = " ".into();
    assert!(exploration.revise(silent).is_err());
    assert_eq!(exploration, before);
}

#[test]
fn no_revision_once_the_reviewers_answer_is_on_its_way() {
    let (mut exploration, _) = asked_again();
    let open = exploration.questions.last().unwrap().clone();
    exploration
        .request(
            Some(AnswerInput {
                option: Some("keep".into()),
                ..AnswerInput::default()
            }),
            Some(&open),
        )
        .unwrap();

    let error = exploration
        .revise(revision(&exploration, 3))
        .unwrap_err()
        .to_string();
    assert!(error.contains("on its way"), "{error}");
    exploration.cancel();
    assert!(exploration.revise(revision(&exploration, 3)).is_err());
}

#[test]
fn cancelling_the_answer_before_a_revised_question_drops_the_revision_too() {
    let (exploration, answer) = asked_again();
    let mut round = ExploreRound::new(exploration);
    let revision = revision(&round.exploration, 3);
    assert!(round.exploration.revise(revision).unwrap());

    round.cancel_answer(&answer.id).unwrap();

    assert_eq!(round.exploration.conversation.len(), 1);
    assert_eq!(round.exploration.questions, vec![question(1)]);
    assert_eq!(
        round.exploration.waiting_turn().unwrap().next,
        Some(question(1))
    );
}

#[test]
fn a_revision_adds_its_terms_and_a_retry_with_other_terms_is_refused() {
    let (mut exploration, _) = asked_again();
    let mut revised = revision(&exploration, 3);
    revised.glossary = [("rebuild".to_owned(), "To make the cache again.".to_owned())].into();

    assert!(exploration.revise(revised.clone()).unwrap());
    assert_eq!(
        exploration.glossary.get("rebuild").map(String::as_str),
        Some("To make the cache again.")
    );
    revised.glossary = [("rebuild".to_owned(), "Another meaning.".to_owned())].into();
    assert!(exploration.revise(revised).is_err());
}
