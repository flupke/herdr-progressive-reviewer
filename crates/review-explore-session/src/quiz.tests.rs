//! The quiz of a conclusion on the Explore page: published with its proofs, and the reviewer's
//! answers saved with the round.

use review_explore::{QuizPick, QuizResponse};
use review_explore_page::{
    CommandRefusal, CommandReply, PageCommand, PageQuiz, PageQuizResponse, RoundStage,
};

use super::*;

/// A quiz of two items, each proven by the line of `reviewed.rs`; the second option is correct.
fn quiz() -> serde_json::Value {
    let item = |question: &str| {
        serde_json::json!({
            "question": question,
            "answers": ["Kept in the pane", "Saved with the round"],
            "correct": 1,
            "why": "The round's record holds it.",
            "proof": [{"path": "reviewed.rs", "side": "new", "lines": {"first_line": 1, "last_line": 1},
                "notes": "The policy lives here."}],
            "level": "Data storage: where the policy is kept."
        })
    };
    serde_json::json!({"quiz": [item("Where is the policy kept?"), item("What survives a restart?")]})
}

impl Harness {
    /// Ask the first question, answer it and let the agent conclude with `quiz()`.
    fn conclude_with_quiz(&mut self) -> TurnRequest {
        self.capture();
        let first = self.request(None);
        let access = self.turn(&first);
        assert!(applied(self.submit(&access, question(&first, 1))));
        let (request, access) = self.answer("Keep it.");
        assert!(applied(self.submit(
            &access,
            conclusion_with_quiz(&request, CONCLUSION, quiz())
        )));
        request
    }

    /// Answer the quiz of the conclusion of the turn `conclusion` from the Explore page, and
    /// return the session's reply.
    fn answer_quiz(
        &mut self,
        conclusion: &str,
        response: QuizResponse,
    ) -> Result<(), CommandRefusal> {
        let (reply, replied) = CommandReply::channel();
        let response = PageQuizResponse {
            conclusion: conclusion.into(),
            response,
        };
        self.session.handle(Input::Page {
            command: PageCommand::Quiz(response),
            reply,
        });
        replied.blocking_recv().expect("the session replies")
    }

    /// The quiz the page shows with the conclusion.
    fn shown_quiz(&self) -> PageQuiz {
        match self.page.stage() {
            RoundStage::Conclusion { quiz, .. } => quiz,
            stage => panic!("the page shows {stage:?}"),
        }
    }
}

#[test]
fn the_page_shows_the_quiz_with_the_lines_of_each_proof() {
    let mut harness = Harness::start();
    harness.conclude_with_quiz();

    let quiz = harness.shown_quiz();

    assert_eq!(quiz.proofs.len(), 2);
    let proof = &quiz.proofs[0][0];
    assert_eq!(proof.evidence.notes, "The policy lives here.");
    let rows = proof.lines.as_ref().expect("the proof shows its lines");
    assert_eq!(rows.len(), 1);
    assert_eq!(quiz.answers, review_explore::QuizAnswers::default());
    assert!(quiz.takes_answers);
}

#[test]
fn the_quiz_of_a_round_left_read_only_asks_nothing() {
    let mut harness = Harness::start();
    let conclusion = harness.conclude_with_quiz().request;
    let mut history = harness.history();
    history.latest_editable = false;
    harness
        .store
        .lock_explore(&harness.unit)
        .unwrap()
        .save_history(&history)
        .unwrap();

    assert!(harness.reopen().historical);

    assert!(!harness.shown_quiz().takes_answers);
    assert_eq!(
        harness.answer_quiz(&conclusion, QuizResponse::Skip),
        Err(CommandRefusal::Stale)
    );
}

#[test]
fn a_pick_from_the_page_is_graded_saved_with_the_round_and_shown() {
    let mut harness = Harness::start();
    let conclusion = harness.conclude_with_quiz().request;
    while harness.events.try_recv().is_ok() {}

    let wrong = QuizResponse::Pick { item: 0, answer: 0 };
    assert_eq!(harness.answer_quiz(&conclusion, wrong), Ok(()));

    let picks = vec![QuizPick {
        item: 0,
        answer: 0,
        correct: false,
    }];
    let saved = harness.saved();
    assert_eq!(
        saved.exploration.quiz_answers(&conclusion).unwrap().picks,
        picks
    );
    let committed: ui_events::ExploreCommitted = harness.next();
    assert_eq!(
        committed.round.exploration.quiz_answers(&conclusion),
        saved.exploration.quiz_answers(&conclusion)
    );
    assert_eq!(harness.shown_quiz().answers.picks, picks);
}

#[test]
fn a_skip_from_the_page_is_saved_and_takes_no_more_picks() {
    let mut harness = Harness::start();
    let conclusion = harness.conclude_with_quiz().request;

    assert_eq!(harness.answer_quiz(&conclusion, QuizResponse::Skip), Ok(()));
    let pick = QuizResponse::Pick { item: 0, answer: 1 };

    assert_eq!(
        harness.answer_quiz(&conclusion, pick),
        Err(CommandRefusal::Stale)
    );
    assert!(harness.shown_quiz().answers.skipped);
    assert!(
        harness
            .saved()
            .exploration
            .quiz_answers(&conclusion)
            .unwrap()
            .skipped
    );
}

#[test]
fn a_second_pick_of_the_same_item_from_another_page_is_stale() {
    let mut harness = Harness::start();
    let conclusion = harness.conclude_with_quiz().request;
    let right = QuizResponse::Pick { item: 0, answer: 1 };
    assert_eq!(harness.answer_quiz(&conclusion, right), Ok(()));
    let revision = harness.saved().revision;

    assert_eq!(
        harness.answer_quiz(&conclusion, right),
        Ok(()),
        "the same pick"
    );
    let other = QuizResponse::Pick { item: 0, answer: 0 };
    assert_eq!(
        harness.answer_quiz(&conclusion, other),
        Err(CommandRefusal::Stale)
    );
    assert_eq!(harness.saved().revision, revision, "nothing to save again");
}

#[test]
fn a_pick_for_a_conclusion_that_is_no_longer_current_is_stale() {
    let mut harness = Harness::start();
    harness.conclude_with_quiz();
    let pick = QuizResponse::Pick { item: 0, answer: 1 };

    assert_eq!(
        harness.answer_quiz("an-earlier-turn", pick),
        Err(CommandRefusal::Stale)
    );
}
