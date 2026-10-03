//! The stage of the round the session publishes for the Explore page.

use review_explore::Command;
use review_explore_page::RoundStage;
use review_repository::diff::DiffRow;

use super::*;

/// The number and text of the question the page shows, or `None` in another stage.
fn shown_question(stage: &RoundStage) -> Option<(usize, String)> {
    match stage {
        RoundStage::Question {
            number, question, ..
        } => Some((*number, question.id.clone())),
        _ => None,
    }
}

#[test]
fn the_page_follows_the_round_from_its_kickoff_to_its_conclusion_and_reset() {
    let mut harness = Harness::start();
    assert_eq!(harness.page.stage(), RoundStage::NoRound);

    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    assert_eq!(harness.page.stage(), RoundStage::AgentWorking);

    assert!(applied(harness.submit(&access, question(&first, 1))));
    assert_eq!(
        shown_question(&harness.page.stage()),
        Some((1, "q1".into()))
    );

    let (answer, access) = harness.answer("Keep it.");
    assert_eq!(harness.page.stage(), RoundStage::AgentWorking);
    assert!(applied(harness.submit(&access, question(&answer, 2))));
    assert_eq!(
        shown_question(&harness.page.stage()),
        Some((2, "q2".into()))
    );

    let (answer, access) = harness.answer("Keep it too.");
    assert!(applied(
        harness.submit(&access, conclusion(&answer, CONCLUSION))
    ));
    let RoundStage::Conclusion(shown) = harness.page.stage() else {
        panic!("the page shows {:?}", harness.page.stage());
    };
    assert_eq!(shown.summary, CONCLUSION);

    harness.session.handle(Input::Command(Command::Reset));
    assert_eq!(harness.page.stage(), RoundStage::NoRound);
}

#[test]
fn the_page_shows_a_turn_the_agent_no_longer_works_on_as_interrupted() {
    let mut harness = Harness::start();
    harness.capture();
    let first = harness.request(None);
    harness.turn(&first);

    harness.session.handle(Input::Command(Command::Cancel));

    assert_eq!(harness.page.stage(), RoundStage::Interrupted);
}

#[test]
fn a_reopened_reviewer_shows_its_restored_round_on_the_page() {
    let mut harness = Harness::start();
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    assert!(applied(harness.submit(&access, question(&first, 1))));

    harness.reopen();
    assert_eq!(
        shown_question(&harness.page.stage()),
        Some((1, "q1".into()))
    );

    harness.answer("Keep it.");
    harness.reopen();
    assert_eq!(
        harness.page.stage(),
        RoundStage::Interrupted,
        "the reopened reviewer no longer waits for the agent's turn"
    );
}

#[test]
fn a_cancelled_answer_brings_its_question_back_to_the_page() {
    let mut harness = Harness::start();
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    assert!(applied(harness.submit(&access, question(&first, 1))));
    let (answer, _) = harness.answer("Keep it.");

    let id = answer.answer.unwrap().id;
    harness
        .session
        .handle(Input::Command(Command::CancelAnswer(id)));

    assert_eq!(
        shown_question(&harness.page.stage()),
        Some((1, "q1".into()))
    );
}

#[test]
fn the_page_tells_each_round_from_the_next() {
    let mut harness = Harness::start();
    assert_eq!(harness.page.round(), None);

    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    let round = harness.page.round();
    assert!(round.is_some());
    assert!(applied(harness.submit(&access, question(&first, 1))));
    assert_eq!(
        harness.page.round(),
        round,
        "the same round asks its question"
    );

    harness.session.handle(Input::Command(Command::Reset));
    assert_eq!(harness.page.round(), None);

    harness.capture();
    let next = harness.request(None);
    harness.turn(&next);
    assert!(harness.page.round().is_some());
    assert_ne!(harness.page.round(), round);
}

#[test]
fn the_page_shows_the_lines_each_citation_of_the_question_names() {
    let mut harness = Harness::start();
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);

    assert!(applied(harness.submit(&access, question(&first, 1))));

    let RoundStage::Question { citations, .. } = harness.page.stage() else {
        panic!("the page shows {:?}", harness.page.stage());
    };
    let [citation] = &citations[..] else {
        panic!("the page shows {} citations", citations.len());
    };
    assert_eq!(citation.evidence.location.to_string(), "reviewed.rs new 1");
    assert_eq!(citation.evidence.notes, "Implements the policy");
    let rows = citation.lines.as_ref().unwrap();
    let shown: Vec<_> = rows
        .iter()
        .map(|row| {
            let text: String = row.tokens.iter().map(|token| token.text.as_str()).collect();
            (row.diff.clone(), text)
        })
        .collect();
    assert_eq!(
        shown,
        [(
            DiffRow::Add {
                new_line: 1,
                text: "+pub fn reviewed() {}".into()
            },
            "pub fn reviewed() {}".into()
        )]
    );
    assert!(
        matches!(
            harness.store.load(&harness.unit, b"reviewed.rs").unwrap(),
            review_store::LoadResult::Unreviewed
        ),
        "showing a citation marks none of its lines"
    );
}
