//! Diagram errors the Explore page reports: saved with their question.

use review_explore::DiagramError;
use review_explore_page::{CommandRefusal, CommandReply, PageCommand};

use super::*;

fn broken(question: &str) -> DiagramError {
    DiagramError {
        question: question.into(),
        version: 1,
        source: "flowchart LR\n  A --> B[label (x)]".into(),
        message: "Parse error on line 2".into(),
    }
}

/// A session whose agent asked its first question, `q1`.
fn asked() -> Harness {
    let mut harness = Harness::start();
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    assert!(applied(harness.submit(&access, question(&first, 1))));
    harness
}

impl Harness {
    /// Report `error` from the Explore page, and return the session's reply.
    fn report_diagram_error(&mut self, error: DiagramError) -> Result<(), CommandRefusal> {
        let (reply, replied) = CommandReply::channel();
        self.session.handle(Input::Page {
            command: PageCommand::DiagramFailed(error),
            reply,
        });
        replied.blocking_recv().expect("the session replies")
    }
}

#[test]
fn a_diagram_error_the_page_reports_is_saved_with_its_question() {
    let mut harness = asked();
    while harness.events.try_recv().is_ok() {}

    assert_eq!(harness.report_diagram_error(broken("q1")), Ok(()));

    assert_eq!(
        harness.saved().exploration.diagram_errors,
        vec![broken("q1")]
    );
    let committed: ui_events::ExploreCommitted = harness.next();
    assert_eq!(
        committed.round.exploration.diagram_errors,
        vec![broken("q1")]
    );
}

#[test]
fn the_same_diagram_error_again_is_accepted_once() {
    let mut harness = asked();

    assert_eq!(harness.report_diagram_error(broken("q1")), Ok(()));
    let revision = harness.saved().revision;
    assert_eq!(harness.report_diagram_error(broken("q1")), Ok(()));

    assert_eq!(harness.saved().revision, revision, "nothing to save again");
}

#[test]
fn a_diagram_error_for_a_question_the_round_never_posted_is_stale() {
    let mut harness = asked();

    assert_eq!(
        harness.report_diagram_error(broken("q9")),
        Err(CommandRefusal::Stale)
    );

    assert!(harness.saved().exploration.diagram_errors.is_empty());
}
