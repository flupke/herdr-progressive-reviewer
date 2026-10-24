//! A bare answer whose fork has not submitted its turn yet: the answer waits for the fork, and
//! continues as it once it submits, or goes to the agent in the pane.

use super::*;

impl Harness {
    /// Asks the first question with a fork for each choice, and answers it with "keep" and no
    /// comment before its fork submitted. Returns the answer's turn, the fork's session and the
    /// turn the fork was told, and the session of the other fork.
    fn answer_before_the_fork_submits(&mut self) -> (TurnRequest, String, TurnRequest, String) {
        self.ask(RunAhead::Every);
        let saved = self.forks_saved();
        let (fork, other) = (
            saved.forks[0].session.clone(),
            saved.forks[1].session.clone(),
        );
        let told = self.fork_request(&fork);
        let request = self.post_answer(Some("keep"), "");
        (request, fork, told, other)
    }

    /// The turns the pane heard wait for their fork, by request, with when the fork started
    /// each, since the test last read the session's events.
    fn finishing_heard(&self) -> Vec<(String, u64)> {
        let mut heard = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            if let Some(event) = event.downcast_ref::<ui_events::ExploreTurnFinishing>() {
                heard.push((event.request.clone(), event.started_at_ms));
            }
        }
        heard
    }
}

#[test]
fn a_bare_answer_waits_for_its_fork_then_the_agent_switches_to_it_once_it_submits() {
    let mut harness = Harness::start();
    let (request, fork, told, other) = harness.answer_before_the_fork_submits();

    assert_eq!(harness.forks.discarded(), [other], "the other fork goes");
    assert!(
        harness.no_prompt_sent(),
        "the agent in the pane gets no prompt"
    );
    assert!(harness.nothing_moved(), "the agent waits for the fork");
    let started = harness.forks_saved().forks[0].taken_at_ms;
    assert_eq!(
        harness.finishing_heard(),
        [(request.request.clone(), started)],
        "the pane hears since when the fork works on the answer's turn"
    );
    let RoundStage::AgentWorking {
        request: working,
        finishing_since_ms,
        ..
    } = harness.page.stage()
    else {
        panic!("the page shows {:?}", harness.page.stage());
    };
    assert_eq!(working, request.request);
    assert_eq!(finishing_since_ms, Some(started));

    assert!(applied(harness.submit_as_fork(&fork, question(&told, 2))));

    assert_eq!(
        harness.forks.switches(),
        std::slice::from_ref(&fork),
        "the agent switches once the fork submitted, before it ends"
    );
    let switched = harness.agent_on(&fork);
    harness.forks.finish_switch(Ok(switched));
    harness.pump();
    assert!(harness.no_prompt_sent());
    let RoundStage::Question { response, .. } = harness.page.stage() else {
        panic!("the page shows the next question");
    };
    assert_eq!(
        response.path,
        Some(TurnPath::Prepared {
            session: fork.clone()
        })
    );
    assert_eq!(
        harness
            .saved()
            .exploration
            .conversation
            .last()
            .unwrap()
            .update,
        turn_of(&question(&request, 2)),
        "the round has the fork's turn"
    );
}

#[test]
fn stop_waiting_for_the_fork_gives_the_answer_to_the_agent_in_the_pane() {
    let mut harness = Harness::start();
    let (request, fork, told, _) = harness.answer_before_the_fork_submits();

    harness.session.handle(Input::Command(Command::Cancel));

    let path = TurnPath::Plain {
        reason: PlainReason::StoppedWaiting,
    };
    assert_eq!(harness.latest_path(), path);
    let saved = harness.forks_saved();
    assert_eq!(
        discarded_for(&saved.forks[0]),
        Some(DiscardReason::StoppedWaiting)
    );
    assert_eq!(
        saved.failed_in_a_row, 0,
        "stopping the wait is no failure of run-ahead"
    );
    assert!(harness.forks.discarded().contains(&fork));
    assert!(
        harness.submit_as_fork(&fork, question(&told, 2)).is_err(),
        "the fork's late turn is refused"
    );
    assert!(harness.nothing_moved());
    assert_eq!(
        harness.agent_takes(&request),
        Some(path),
        "the agent in the pane takes the turn from the start"
    );
}

#[test]
fn a_fork_that_ends_without_its_turn_gives_the_answer_to_the_agent_in_the_pane() {
    let mut harness = Harness::start();
    let (request, fork, _, _) = harness.answer_before_the_fork_submits();

    harness.forks.end(&fork);
    harness.pump();

    let path = TurnPath::Plain {
        reason: PlainReason::NoTurn,
    };
    assert_eq!(harness.latest_path(), path);
    assert!(harness.nothing_moved());
    assert_eq!(harness.agent_takes(&request), Some(path));
}

#[test]
fn a_chat_message_written_while_the_answer_waits_gives_it_to_the_agent_once_the_fork_submits() {
    let mut harness = Harness::start();
    let (request, fork, told, _) = harness.answer_before_the_fork_submits();
    harness.write_in_chat("Why does the policy exist?");

    assert!(applied(harness.submit_as_fork(&fork, question(&told, 2))));

    let path = TurnPath::Plain {
        reason: PlainReason::ChatMessage,
    };
    assert_eq!(harness.latest_path(), path);
    assert!(harness.nothing_moved(), "the agent stays on its session");
    assert!(harness.forks.discarded().contains(&fork));
    assert_eq!(harness.agent_takes(&request), Some(path));
}

#[test]
fn cancelling_the_answer_that_waits_discards_its_fork_and_forks_the_question_again() {
    let mut harness = Harness::start();
    let (request, fork, told, _) = harness.answer_before_the_fork_submits();

    harness.session.handle(Input::Command(Command::CancelAnswer(
        request.answer.unwrap().id,
    )));

    let saved = harness.forks_saved();
    assert_eq!(
        discarded_for(&saved.forks[0]),
        Some(DiscardReason::Withdrawn)
    );
    assert!(harness.forks.discarded().contains(&fork));
    assert!(harness.submit_as_fork(&fork, question(&told, 2)).is_err());
    assert!(harness.no_prompt_sent() && harness.nothing_moved());
    assert_eq!(
        harness.forks.started().len(),
        4,
        "the question waits again, with new forks"
    );
}

#[test]
fn stop_waiting_while_the_agent_switches_to_the_fork_gives_the_answer_to_the_agent_on_its_session()
{
    let mut harness = Harness::start();
    let (request, fork, told, _) = harness.answer_before_the_fork_submits();
    assert!(applied(harness.submit_as_fork(&fork, question(&told, 2))));
    assert_eq!(harness.forks.switches(), std::slice::from_ref(&fork));

    harness.session.handle(Input::Command(Command::Cancel));

    assert!(
        harness.no_prompt_sent(),
        "no prompt while the agent switches"
    );
    harness.forks.finish_switch(Ok(harness.agent_on(&fork)));
    harness.pump();
    assert_eq!(
        harness.forks.resumes(),
        ["conversation"],
        "the round takes no fork's turn: the agent goes back to its own session"
    );
    assert!(harness.no_prompt_sent(), "no prompt while it goes back");
    harness
        .forks
        .finish_resume(Ok(harness.agent_on("conversation")));
    harness.pump();
    let path = TurnPath::Plain {
        reason: PlainReason::StoppedWaiting,
    };
    assert_eq!(harness.latest_path(), path);
    assert_eq!(harness.agent_takes(&request), Some(path));
}

#[test]
fn stop_waiting_on_the_page_tells_the_pane_before_the_answer_goes_to_the_agent() {
    let mut harness = Harness::start();
    let (request, _, _, _) = harness.answer_before_the_fork_submits();
    while harness.events.try_recv().is_ok() {}

    harness.stop_on_page().unwrap();

    let mut heard = Vec::new();
    while let Ok(event) = harness.events.try_recv() {
        if event
            .downcast_ref::<ui_events::ExplorePageStopped>()
            .is_some()
        {
            heard.push("stopped");
        } else if event.downcast_ref::<ui_events::ExplorePosted>().is_some() {
            heard.push("posted");
        }
    }
    assert_eq!(
        heard,
        ["stopped", "posted"],
        "the pane offers Retry only until it hears that the answer went out again"
    );
    assert!(Harness::is_prompt_of(&harness.delivered_prompt(), &request));
}

#[test]
fn a_reviewer_that_closes_while_the_answer_waits_records_no_prepared_turn() {
    let mut harness = Harness::start();
    harness.answer_before_the_fork_submits();

    harness.reopen();

    assert_eq!(
        harness.latest_path(),
        TurnPath::Plain {
            reason: PlainReason::NoTurn
        }
    );
    assert_eq!(
        discarded_for(&harness.forks_saved().forks[0]),
        Some(DiscardReason::ReviewerClosed)
    );
}

#[test]
fn an_answer_whose_unreviewed_lines_changed_does_not_wait_for_its_fork() {
    let mut harness = Harness::start();
    harness.files.write("other.rs", b"pub fn other() {}\n");
    harness.ask(RunAhead::Every);
    let snapshot = complete_repository_snapshot(&harness.repository);
    let other = snapshot
        .files
        .iter()
        .find(|file| file.review_path().display() == "other.rs")
        .unwrap();
    review_state::ReviewTracker::new(harness.repository.clone(), harness.store.clone())
        .mark(&snapshot, other, &review_types::MarkAuthor::Reviewer)
        .unwrap();

    let request = harness.post_answer(Some("keep"), "");

    let path = TurnPath::Plain {
        reason: PlainReason::UnreviewedChanged,
    };
    assert_eq!(harness.latest_path(), path);
    assert_eq!(harness.forks.discarded().len(), 2, "no fork is waited for");
    assert_eq!(harness.agent_takes(&request), Some(path));
}
