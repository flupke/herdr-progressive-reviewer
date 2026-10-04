//! A review whose changed lines are all marked as reviewed cannot start a round, from the pane,
//! from the page or through the session directly; a round already running goes on.

use review_explore::StartBlock;
use review_explore_page::{CommandRefusal, CommandReply, PageCommand, RoundStage};
use review_state::ReviewState;
use review_types::MarkAuthor;

use super::*;

impl Harness {
    /// Mark every changed file as reviewed, as another component of the reviewer would; returns
    /// the review states the marks leave.
    fn review_everything(&self) -> Vec<ReviewState> {
        self.mark_every_file(true)
    }

    /// Unmark every changed file; returns the review states left.
    fn unreview_everything(&self) -> Vec<ReviewState> {
        self.mark_every_file(false)
    }

    fn mark_every_file(&self, reviewed: bool) -> Vec<ReviewState> {
        let tracker = review_state::ReviewTracker::new(self.repository.clone(), self.store.clone());
        let snapshot = complete_repository_snapshot(&self.repository);
        for file in &snapshot.files {
            if reviewed {
                tracker
                    .mark(&snapshot, file, &MarkAuthor::Reviewer)
                    .unwrap();
            } else {
                tracker.unreview(&snapshot, file).unwrap();
            }
        }
        tracker.statuses(&snapshot).unwrap()
    }

    /// Start a round from the page, as the reviewer's worker does; return the reply.
    fn start_from_page(&mut self) -> Result<(), CommandRefusal> {
        let (reply, replied) = CommandReply::channel();
        if let Some(kickoff) = self.session.start_from_page(false, reply) {
            self.session
                .handle(Input::Command(Command::Turn(Box::new(kickoff))));
        }
        replied.blocking_recv().expect("the session replies")
    }
}

fn nothing_to_review() -> String {
    StartBlock::NothingToReview.reason().to_owned()
}

#[test]
fn the_pane_and_the_page_hear_whether_the_review_marks_leave_something_to_review() {
    let mut harness = Harness::start();

    let reviewed = harness.review_everything();
    harness.session.marks_changed(&reviewed);

    assert_eq!(
        harness.next::<ui_events::ExploreStartBlock>().0,
        Some(StartBlock::NothingToReview)
    );
    assert_eq!(
        harness.page.start_block(),
        Some(StartBlock::NothingToReview)
    );

    let unreviewed = harness.unreview_everything();
    harness.session.marks_changed(&unreviewed);

    assert_eq!(harness.next::<ui_events::ExploreStartBlock>().0, None);
    assert_eq!(harness.page.start_block(), None);
}

#[test]
fn a_fully_reviewed_review_cannot_start_a_round_through_the_session() {
    let mut harness = Harness::start();
    harness.review_everything();

    harness.session.handle(Input::Command(Command::Start));

    let captured = harness.next::<ui_events::ExploreCaptured>();
    assert_eq!(captured.result.err(), Some(nothing_to_review()));
    assert_eq!(
        harness.page.stage(),
        RoundStage::StartFailed {
            failure: nothing_to_review()
        }
    );
    assert!(harness.agents.prompts().is_empty());
}

#[test]
fn a_fully_reviewed_review_cannot_start_a_round_from_the_page() {
    let mut harness = Harness::start();
    let reviewed = harness.review_everything();
    harness.session.marks_changed(&reviewed);

    assert_eq!(
        harness.start_from_page(),
        Err(CommandRefusal::Failed(nothing_to_review()))
    );
    assert_eq!(harness.page.stage(), RoundStage::NoRound);
    assert!(harness.agents.prompts().is_empty());

    // Once a line is unreviewed, the same session starts one.
    let unreviewed = harness.unreview_everything();
    harness.session.marks_changed(&unreviewed);
    assert_eq!(harness.start_from_page(), Ok(()));
    assert_eq!(harness.page.stage(), RoundStage::AgentWorking);
    assert!(harness.delivered_prompt().contains("Explore request: "));
}

#[test]
fn a_start_that_finds_nothing_left_to_review_sends_no_kickoff() {
    // Jev, or another reviewer, marks the rest between the capture and the kickoff: from the
    // pane, and from the page.
    for from_page in [false, true] {
        let mut harness = Harness::start();
        let kickoff = if from_page {
            let (reply, replied) = CommandReply::channel();
            let kickoff = harness.session.start_from_page(false, reply);
            assert_eq!(replied.blocking_recv().unwrap(), Ok(()));
            kickoff.expect("the kickoff, for the worker to send")
        } else {
            harness.capture();
            harness.request(None)
        };
        harness.review_everything();

        harness
            .session
            .handle(Input::Command(Command::Turn(Box::new(kickoff))));

        let posted = harness.next::<ui_events::ExplorePosted>();
        assert_eq!(posted.result.err(), Some(nothing_to_review()));
        assert_eq!(
            harness.page.stage(),
            RoundStage::StartFailed {
                failure: nothing_to_review()
            }
        );
        assert!(harness.agents.prompts().is_empty(), "no kickoff");
        assert!(
            harness.history().restorable().is_none(),
            "no round is saved"
        );
    }
}

#[test]
fn a_running_round_goes_on_once_its_last_line_is_marked() {
    let mut harness = Harness::start();
    harness.capture();
    let first = harness.request(None);
    let access = harness.turn(&first);
    assert!(applied(harness.submit(&access, question(&first, 1))));

    let reviewed = harness.review_everything();
    harness.session.marks_changed(&reviewed);
    harness.answer("Keep it.");

    assert_eq!(
        harness.agents.prompts().len(),
        2,
        "the answer reaches the agent"
    );
}

#[test]
fn a_page_post_does_not_bypass_the_rule() {
    let mut harness = Harness::start();
    harness.review_everything();

    let (reply, replied) = CommandReply::channel();
    harness.session.handle(Input::Page {
        command: PageCommand::Start { challenger: false },
        reply,
    });

    // The session learns of the marks only as it captures the change.
    assert_eq!(replied.blocking_recv().unwrap(), Ok(()));
    assert_eq!(
        harness.page.stage(),
        RoundStage::StartFailed {
            failure: nothing_to_review()
        }
    );
    assert!(harness.agents.prompts().is_empty());
}
