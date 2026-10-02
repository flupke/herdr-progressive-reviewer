//! Cancelling the latest answer gives back its turn's marks and rewinds the round.

use review_explore::Command;
use review_store::LoadResult;
use review_types::MarkAuthor;

use super::*;

/// A conclusion after `request`'s answer that marks the only changed line
/// reviewed, taking it over from whoever had marked it.
fn retaking_conclusion(request: &TurnRequest) -> Operation {
    let Operation::SubmitConclusion(mut submission) = marking_conclusion(request) else {
        unreachable!("a conclusion");
    };
    submission.reopened = submission.reviewed.clone();
    Operation::SubmitConclusion(submission)
}

impl Harness {
    /// The kickoff and first question, then an answer awaiting the agent.
    fn answered(&mut self) -> (TurnRequest, String) {
        self.capture();
        let first = self.request(None);
        let access = self.turn(&first);
        assert!(applied(self.submit(&access, question(&first, 1))));
        self.answer("Keep it.")
    }

    fn cancel(&mut self, answer: &str) -> Result<Arc<ExploreRound>, String> {
        self.session
            .handle(Input::Command(Command::CancelAnswer(answer.to_owned())));
        let cancelled = self.next::<ui_events::ExploreAnswerCancelled>();
        assert_eq!(cancelled.answer, answer);
        if let Ok(round) = &cancelled.result {
            self.exploration = Some(round.exploration.clone());
        }
        cancelled.result
    }

    fn author(&self) -> Option<MarkAuthor> {
        match self.store.load(&self.unit, b"reviewed.rs").unwrap() {
            LoadResult::Reviewed(record) => Some(record.author),
            _ => None,
        }
    }

    fn mark_as_reviewer(&self) {
        let snapshot = complete_repository_snapshot(&self.repository);
        let tracker = review_state::ReviewTracker::new(self.repository.clone(), self.store.clone());
        tracker
            .mark(&snapshot, &snapshot.files[0], &MarkAuthor::Reviewer)
            .unwrap();
    }
}

#[test]
fn cancelling_an_answer_reopens_its_lines_and_the_next_prompt_says_so() {
    let mut harness = Harness::start();
    let (request, access) = harness.answered();
    assert!(applied(
        harness.submit(&access, marking_conclusion(&request))
    ));
    let answer = request.answer.unwrap().id;
    assert!(harness.author().is_some());

    let round = harness.cancel(&answer).unwrap();

    assert_eq!(harness.author(), None);
    assert!(round.exploration.answers.is_empty() && round.exploration.conclusion.is_none());
    assert!(round.marks.is_empty() && round.completion.is_none());
    assert_eq!(harness.saved(), *round);
    let (replacement, _) = harness.answer("Keep it, with a test.");
    assert_eq!(replacement.cancelled, vec![answer.clone()]);
    let prompt = harness.agents.prompts().last().unwrap().text.clone();
    assert!(
        prompt.contains(&format!("Cancelled answer: {answer}\n")),
        "{prompt}"
    );
}

#[test]
fn lines_the_answer_took_over_return_to_their_previous_author() {
    let mut harness = Harness::start();
    harness.mark_as_reviewer();
    let (request, access) = harness.answered();
    assert!(applied(
        harness.submit(&access, retaking_conclusion(&request))
    ));
    let answer = request.answer.unwrap().id;
    assert_eq!(
        harness.author(),
        Some(MarkAuthor::Explore {
            answer: answer.clone()
        })
    );

    harness.cancel(&answer).unwrap();

    assert_eq!(harness.author(), Some(MarkAuthor::Reviewer));
}

#[test]
fn an_answer_cancelled_mid_turn_rejects_the_agents_late_reply() {
    let mut harness = Harness::start();
    let (request, access) = harness.answered();
    let answer = request.answer.as_ref().unwrap().id.clone();

    harness.cancel(&answer).unwrap();

    assert!(
        harness
            .submit(&access, conclusion(&request, CONCLUSION))
            .is_err()
    );
    let saved = harness.saved();
    assert!(saved.exploration.answers.is_empty() && saved.turns.len() == 1);
}

#[test]
fn only_the_latest_answer_can_be_cancelled() {
    let mut harness = Harness::start();
    let (request, access) = harness.answered();
    assert!(applied(harness.submit(&access, question(&request, 2))));
    let (_, _) = harness.answer("Second answer.");
    let first = request.answer.unwrap().id;

    let error = harness.cancel(&first).unwrap_err();

    assert!(error.contains("Only the latest answer"), "{error}");
    assert_eq!(harness.saved().exploration.answers.len(), 2);
}

#[test]
fn cancelling_an_answer_also_reopens_what_its_turn_found_not_relevant() {
    let mut harness = Harness::start();
    let (request, access) = harness.answered();
    let Operation::SubmitConclusion(mut submission) = conclusion(&request, CONCLUSION) else {
        unreachable!("a conclusion");
    };
    submission.not_relevant = vec![
        serde_json::from_value(serde_json::json!({
            "path": "reviewed.rs", "side": "new", "lines": null
        }))
        .unwrap(),
    ];
    assert!(applied(
        harness.submit(&access, Operation::SubmitConclusion(submission))
    ));
    assert!(harness.author().is_some());

    harness.cancel(&request.answer.unwrap().id).unwrap();

    assert_eq!(harness.author(), None);
}

#[test]
fn lines_found_not_relevant_after_an_answer_carry_that_answers_author() {
    let mut harness = Harness::start();
    let (request, access) = harness.answered();
    let Operation::SubmitConclusion(mut submission) = conclusion(&request, CONCLUSION) else {
        unreachable!("a conclusion");
    };
    submission.not_relevant = vec![
        serde_json::from_value(serde_json::json!({
            "path": "reviewed.rs", "side": "new", "lines": {"first_line": 1, "last_line": 1}
        }))
        .unwrap(),
    ];

    assert!(applied(
        harness.submit(&access, Operation::SubmitConclusion(submission))
    ));

    let answer = request.answer.unwrap().id;
    let marks = &harness.saved().marks[&request.request];
    assert_eq!(marks.counts().not_relevant_lines, 1);
    assert!(marks.reviewed.is_empty());
    assert_eq!(harness.author(), Some(MarkAuthor::Explore { answer }));
}
