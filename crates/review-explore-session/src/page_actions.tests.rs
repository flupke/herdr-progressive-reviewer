//! The actions the Explore page offers to recover or close a round, as the pane offers them:
//! Stop waiting, Retry, Cancel answer, Reset, a reply to the conclusion, and the recoveries of an
//! implementation request.

use review_explore::{Command, DispatchState};
use review_explore_page::{
    CommandRefusal, CommandReply, ImplementationState, Interruption, PageCommand, Recovery,
    RoundStage, Waiting,
};

use super::*;

impl Harness {
    /// Send `command` from the Explore page, and return the session's reply.
    fn on_page(&mut self, command: PageCommand) -> Result<(), CommandRefusal> {
        let (reply, replied) = CommandReply::channel();
        self.session.handle(Input::Page { command, reply });
        replied.blocking_recv().expect("the session replies")
    }

    /// The turn the page shows the agent working on.
    fn working_on(&self) -> String {
        match self.page.stage() {
            RoundStage::AgentWorking { request, .. } => request,
            stage => panic!("the page shows {stage:?}"),
        }
    }

    /// The turn the page offers to retry, and why the agent is not working on it.
    fn interrupted(&self) -> (Option<String>, Interruption) {
        match self.page.stage() {
            RoundStage::Interrupted {
                request,
                interruption,
                ..
            } => (request, interruption),
            stage => panic!("the page shows {stage:?}"),
        }
    }

    /// Stop waiting on the page for the turn the page shows the agent working on.
    fn stop_on_page(&mut self) -> Result<(), CommandRefusal> {
        let request = self.working_on();
        self.on_page(PageCommand::Recover(Recovery::Stop(Waiting::Turn(request))))
    }

    /// Stop waiting on the page for the start the page shows under way, if any.
    fn stop_start_on_page(&mut self) -> Result<(), CommandRefusal> {
        let start = match self.page.stage() {
            RoundStage::Starting { start, .. } => start,
            _ => "no-start".into(),
        };
        self.on_page(PageCommand::Recover(Recovery::Stop(Waiting::Start(start))))
    }

    /// The latest attempt of the turn the page offers to retry, as the page shows it.
    fn attempt(&self) -> String {
        match self.page.stage() {
            RoundStage::Interrupted {
                attempt: Some(attempt),
                ..
            } => attempt,
            _ => "no-attempt".into(),
        }
    }
}

/// `prompt` without its access value, which each prompt renews, and the directory of its
/// unreviewed diffs, which each prompt writes anew.
fn without_access(prompt: &str) -> String {
    prompt
        .lines()
        .filter(|line| {
            !line.starts_with("Explore review access: ") && !line.starts_with("Unreviewed diffs: ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn stop_waiting_on_the_page_saves_what_stop_waiting_in_the_pane_saves() {
    let mut harness = Harness::start();
    harness.ask_first_question();
    let (answer, _) = harness.answer("Keep it.");
    let mut pane = Harness::start();
    pane.ask_first_question();
    pane.answer("Keep it.");
    pane.session.handle(Input::Command(Command::Cancel));

    assert_eq!(harness.stop_on_page(), Ok(()));

    let (saved, expected) = (harness.saved().exploration, pane.saved().exploration);
    assert_eq!(saved.pending_request(), None);
    assert_eq!(expected.pending_request(), None);
    assert_eq!(
        saved.retry_request().map(|retry| &retry.request),
        Some(&answer.request)
    );
    assert_eq!(
        harness.interrupted(),
        (Some(answer.request.clone()), Interruption::Stopped)
    );
    assert_eq!(pane.interrupted().1, Interruption::Stopped);
    assert_eq!(
        harness.next::<ui_events::ExplorePageStopped>().round,
        Some(answer.instance),
        "the pane hears of it"
    );
}

#[test]
fn a_stop_for_a_turn_the_agent_no_longer_works_on_is_refused() {
    let mut harness = Harness::start();
    harness.ask_first_question();
    let (answer, access) = harness.answer("Keep it.");

    let other = harness.on_page(PageCommand::Recover(Recovery::Stop(Waiting::Turn(
        "another-turn".into(),
    ))));
    let start = harness.stop_start_on_page();
    assert!(applied(harness.submit(&access, question(&answer, 2))));
    let after_the_reply = harness.on_page(PageCommand::Recover(Recovery::Stop(Waiting::Turn(
        answer.request.clone(),
    ))));

    assert_eq!(other, Err(CommandRefusal::Stale));
    assert_eq!(start, Err(CommandRefusal::Stale), "no start is under way");
    assert_eq!(after_the_reply, Err(CommandRefusal::Stale));
    assert!(matches!(harness.page.stage(), RoundStage::Question { .. }));
}

#[test]
fn stop_waiting_on_the_page_drops_a_round_that_is_starting() {
    let mut harness = Harness::start();
    let (reply, _kickoff) = harness.start_as_worker();
    assert_eq!(reply, Ok(()));

    assert_eq!(harness.stop_start_on_page(), Ok(()));

    assert!(matches!(harness.page.stage(), RoundStage::NoRound { .. }));
    assert_eq!(harness.next::<ui_events::ExplorePageStopped>().round, None);
}

#[test]
fn a_kickoff_the_pane_posts_after_a_stop_on_the_page_starts_nothing() {
    let mut harness = Harness::start();
    // The pane started the round, and posts its kickoff once it hears of the capture; the
    // page's Stop waiting arrives first.
    harness.capture();
    let kickoff = harness.request(None);
    assert_eq!(harness.stop_start_on_page(), Ok(()));

    harness
        .session
        .handle(Input::Command(Command::Turn(Box::new(kickoff))));

    assert!(harness.next::<ui_events::ExplorePosted>().result.is_err());
    assert!(matches!(harness.page.stage(), RoundStage::NoRound { .. }));
    assert!(harness.agents.prompts().is_empty(), "no kickoff was sent");
}

#[test]
fn retry_on_the_page_sends_the_prompt_retry_in_the_pane_sends() {
    let mut harness = Harness::start();
    harness.ask_first_question();
    let (answer, _) = harness.answer("Keep it.");
    let first = harness.agents.prompts().last().unwrap().text.clone();
    assert_eq!(harness.stop_on_page(), Ok(()));

    assert_eq!(
        harness.on_page(PageCommand::Recover(Recovery::Retry {
            request: answer.request.clone(),
            attempt: harness.attempt(),
        })),
        Ok(())
    );

    let posted = harness.next::<ui_events::ExplorePosted>();
    assert_eq!(posted.request.request, answer.request);
    assert!(posted.result.is_ok(), "{:?}", posted.result);
    assert_eq!(
        without_access(&harness.delivered_prompt()),
        without_access(&first)
    );
    assert_eq!(harness.working_on(), answer.request);
    let saved = harness.saved().exploration;
    assert_eq!(saved.answers.len(), 1, "the same answer, sent again");
}

#[test]
fn stop_waiting_then_retry_on_the_page_sends_a_kickoff_again() {
    let mut harness = Harness::start();
    harness.capture();
    let kickoff = harness.request(None);
    harness.turn(&kickoff);
    // The agent never starts on the kickoff: the page shows it working until the reviewer
    // stops waiting.
    assert_eq!(harness.stop_on_page(), Ok(()));
    assert_eq!(
        harness.interrupted(),
        (Some(kickoff.request.clone()), Interruption::Stopped)
    );

    let retried = harness.on_page(PageCommand::Recover(Recovery::Retry {
        request: kickoff.request.clone(),
        attempt: harness.attempt(),
    }));

    assert_eq!(retried, Ok(()));
    let prompt = harness.delivered_prompt();
    assert!(prompt.contains(&format!("Explore request: {}\n", kickoff.request)));
    assert_eq!(harness.working_on(), kickoff.request);
}

#[test]
fn retry_on_the_page_recovers_a_prompt_that_could_not_be_delivered() {
    let mut harness = Harness::start();
    harness.ask_first_question();
    harness.agents.remove_agent(&PaneId(PANE.into()));
    let answer = harness.request(Some(AnswerInput {
        option: Some("keep".into()),
        text: "Keep it.".into(),
        ..AnswerInput::default()
    }));
    harness
        .session
        .handle(Input::Command(Command::Turn(Box::new(answer.clone()))));
    let (request, interruption) = harness.interrupted();
    assert_eq!(request.as_ref(), Some(&answer.request));
    assert!(
        matches!(interruption, Interruption::Failed(_)),
        "{interruption:?}"
    );
    harness.agents.upsert_agent(agent());

    let retried = harness.on_page(PageCommand::Recover(Recovery::Retry {
        request: answer.request.clone(),
        attempt: harness.attempt(),
    }));

    assert_eq!(retried, Ok(()));
    assert!(
        harness
            .delivered_prompt()
            .contains(&format!("Explore request: {}\n", answer.request))
    );
    assert_eq!(harness.working_on(), answer.request);
}

#[test]
fn a_retry_of_a_turn_that_is_not_interrupted_sends_nothing() {
    let mut harness = Harness::start();
    harness.ask_first_question();
    let (answer, _) = harness.answer("Keep it.");
    let prompts = harness.agents.prompts().len();

    let while_working = harness.on_page(PageCommand::Recover(Recovery::Retry {
        request: answer.request.clone(),
        attempt: harness.attempt(),
    }));
    assert_eq!(harness.stop_on_page(), Ok(()));
    let other = harness.on_page(PageCommand::Recover(Recovery::Retry {
        request: "another-turn".into(),
        attempt: harness.attempt(),
    }));

    // The turn is on its way, as after a Retry that went through.
    assert_eq!(while_working, Err(CommandRefusal::AlreadyApplied));
    assert_eq!(other, Err(CommandRefusal::Stale));
    assert_eq!(harness.agents.prompts().len(), prompts);
}

#[test]
fn a_repeated_retry_sends_the_turn_once_and_a_retry_of_an_earlier_attempt_none() {
    let mut harness = Harness::start();
    harness.ask_first_question();
    let (answer, _) = harness.answer("Keep it.");
    assert_eq!(harness.stop_on_page(), Ok(()));
    let attempt = harness.attempt();
    let retry = |harness: &mut Harness, attempt: &str| {
        harness.on_page(PageCommand::Recover(Recovery::Retry {
            request: answer.request.clone(),
            attempt: attempt.to_owned(),
        }))
    };

    assert_eq!(retry(&mut harness, &attempt), Ok(()));
    harness.delivered_prompt();
    let prompts = harness.agents.prompts().len();
    assert_eq!(
        retry(&mut harness, &attempt),
        Err(CommandRefusal::AlreadyApplied)
    );
    assert_eq!(harness.stop_on_page(), Ok(()));
    assert_ne!(harness.attempt(), attempt, "the Retry made a new attempt");
    assert_eq!(retry(&mut harness, &attempt), Err(CommandRefusal::Stale));
    assert_eq!(harness.agents.prompts().len(), prompts);
}

#[test]
fn the_page_offers_to_cancel_the_latest_answer_as_the_pane_does() {
    let mut harness = Harness::start();
    harness.ask_first_question();
    assert_eq!(harness.page.cancellable(), None, "no answer yet");
    let (answer, access) = harness.answer("Keep it.");
    let latest = answer.answer.clone().unwrap();

    let shown = harness.page.cancellable().expect("the latest answer");
    assert_eq!(
        (
            shown.id.as_str(),
            shown.choice.as_deref(),
            shown.comment.as_str()
        ),
        (latest.id.as_str(), Some("Keep it"), "Keep it.")
    );
    assert!(applied(harness.submit(&access, question(&answer, 2))));
    assert_eq!(
        harness.page.cancellable().map(|answer| answer.id),
        Some(latest.id),
        "after the agent's next turn too"
    );
}

#[test]
fn cancel_answer_on_the_page_saves_what_cancel_answer_in_the_pane_saves() {
    let mut harness = Harness::start();
    harness.ask_first_question();
    let (answer, access) = harness.answer("Keep it.");
    assert!(applied(harness.submit(&access, question(&answer, 2))));
    let id = answer.answer.unwrap().id;

    let stale = harness.on_page(PageCommand::Recover(Recovery::CancelAnswer {
        answer: "another-answer".into(),
    }));
    let cancelled = harness.on_page(PageCommand::Recover(Recovery::CancelAnswer {
        answer: id.clone(),
    }));

    assert_eq!(stale, Err(CommandRefusal::Stale));
    assert_eq!(cancelled, Ok(()));
    let event = harness.next::<ui_events::ExploreAnswerCancelled>();
    assert_eq!(event.answer, id);
    assert!(
        event.result.is_ok(),
        "the pane hears of it: {:?}",
        event.result
    );
    let saved = harness.saved().exploration;
    assert!(saved.answers.is_empty());
    assert_eq!(saved.questions.len(), 1);
    assert!(matches!(
        harness.page.stage(),
        RoundStage::Question {
            answer_cancelled: true,
            ..
        }
    ));
    assert_eq!(harness.page.cancellable(), None);
}

#[test]
fn an_answer_is_no_longer_offered_for_cancel_once_implementation_was_requested() {
    let mut harness = Harness::start();
    harness.conclude();
    assert!(harness.page.cancellable().is_some());
    let conclusion = harness
        .saved()
        .exploration
        .conclusion_request()
        .unwrap()
        .to_owned();
    let request = harness
        .saved()
        .exploration
        .implementation("Do it.".into())
        .unwrap();

    harness
        .session
        .handle(Input::Command(Command::Implement(request)));

    assert_eq!(harness.page.cancellable(), None);
    assert!(harness.saved().exploration.conclusion_request() == Some(conclusion.as_str()));
}

#[test]
fn reset_on_the_page_closes_the_round_as_reset_in_the_pane() {
    let mut harness = Harness::start();
    harness.ask_first_question();
    let round = harness.page.round().expect("a running round");

    let stale = harness.on_page(PageCommand::Recover(Recovery::Reset {
        round: "another-round".into(),
    }));
    let reset = harness.on_page(PageCommand::Recover(Recovery::Reset {
        round: round.clone(),
    }));

    assert_eq!(stale, Err(CommandRefusal::Stale));
    assert_eq!(reset, Ok(()));
    assert!(matches!(harness.page.stage(), RoundStage::NoRound { .. }));
    assert_eq!(harness.page.round(), None);
    assert_eq!(harness.next::<ui_events::ExplorePageReset>().round, round);
    assert!(
        matches!(harness.reopen().result, Ok(None)),
        "reopening shows the start screen"
    );
    assert_eq!(harness.start_on_page(false), Ok(()), "a new round starts");
}

#[test]
fn the_page_cancels_an_implementation_request_that_is_being_sent() {
    let mut harness = Harness::start();
    harness.conclude();
    let conclusion = harness.conclusion_request();
    harness.delivery.close();
    assert_eq!(
        harness.on_page(PageCommand::Implement(review_explore_page::PageImplement {
            conclusion: conclusion.clone(),
            replaces: None,
            text: "Do it.".into(),
        })),
        Ok(())
    );
    let shown = harness.shown_implementation().expect("a request");
    assert_eq!(shown.state, ImplementationState::Sending);

    let stale = harness.on_page(PageCommand::Recover(Recovery::CancelImplementation {
        delivery: "another-request".into(),
    }));
    let cancelled = harness.on_page(PageCommand::Recover(Recovery::CancelImplementation {
        delivery: shown.delivery.clone(),
    }));
    harness.delivery.open();

    assert_eq!(stale, Err(CommandRefusal::Stale));
    assert_eq!(cancelled, Ok(()));
    assert_eq!(
        harness.implementation_finished().state,
        DispatchState::Cancelled
    );
    assert_eq!(
        harness.shown_implementation().map(|shown| shown.state),
        Some(ImplementationState::Cancelled)
    );
}

#[test]
fn the_page_sends_a_saved_request_that_a_reopening_paused() {
    let mut harness = Harness::start();
    harness.conclude();
    let conclusion = harness.conclusion_request();
    harness.delivery.close();
    assert_eq!(
        harness.implement_on_page(&conclusion, None, "Do it."),
        Ok(())
    );
    harness.reopen();
    let paused = harness.shown_implementation().expect("a request");
    assert_eq!(paused.state, ImplementationState::Paused);

    let sent = harness.on_page(PageCommand::Recover(Recovery::ResendImplementation {
        conclusion: conclusion.clone(),
        delivery: paused.delivery.clone(),
        attempt: paused.attempt.clone(),
    }));

    assert_eq!(sent, Ok(()));
    harness.delivery.open();
    // The earlier process's prompt ends cancelled; this one's reaches the agent.
    let finished = (0..2)
        .map(|_| harness.implementation_finished())
        .find(|finished| finished.state == DispatchState::Delivered)
        .expect("the saved request reaches the agent");
    assert_eq!(
        (
            finished.request.delivery.as_str(),
            finished.request.text.as_str()
        ),
        (paused.delivery.as_str(), "Do it.")
    );
    let again = harness.on_page(PageCommand::Recover(Recovery::ResendImplementation {
        conclusion,
        delivery: paused.delivery,
        attempt: paused.attempt,
    }));
    // The agent received it: a repeat of the resend sends nothing.
    assert_eq!(again, Err(CommandRefusal::AlreadyApplied));
}

#[test]
fn an_earlier_round_shows_as_such_on_the_page() {
    let mut harness = Harness::start();
    harness.ask_first_question();
    assert!(!harness.page.earlier());
    let mut history = harness.history();
    history.latest_editable = false;
    harness
        .store
        .lock_explore(&harness.unit)
        .unwrap()
        .save_history(&history)
        .unwrap();

    assert!(harness.reopen().historical);

    assert!(harness.page.earlier());
    assert_eq!(harness.page.cancellable(), None);
}

#[test]
fn the_page_says_when_the_tool_cannot_save_its_rounds() {
    let mut harness = Harness::start();
    harness.ask_first_question();
    harness.answer("Keep it.");
    std::fs::write(harness.round_path(), b"unreadable").unwrap();

    harness.session.handle(Input::Command(Command::Cancel));

    assert!(
        matches!(harness.page.stage(), RoundStage::StorageFailed { .. }),
        "the page shows {:?}",
        harness.page.stage()
    );
}

#[test]
fn retry_on_the_page_sends_a_request_the_agent_did_not_start_on_again_as_it_was() {
    let mut harness = Harness::start();
    harness.conclude();
    let conclusion = harness.conclusion_request();
    harness.agents.swallow_prompts(&PaneId(PANE.into()), true);
    assert_eq!(
        harness.implement_on_page(&conclusion, None, "Do it."),
        Ok(())
    );
    let first = harness.implementation_finished();
    assert_eq!(first.state, DispatchState::NotStarted);
    let shown = harness.shown_implementation().expect("a request");
    assert_eq!(shown.state, ImplementationState::NotStarted);
    assert_eq!(
        harness.implement_on_page(&conclusion, Some(&shown.delivery), "Something else."),
        Err(CommandRefusal::Stale),
        "the list may still wait in the agent's prompt box: no other request"
    );
    harness.agents.swallow_prompts(&PaneId(PANE.into()), false);

    let retried = harness.on_page(PageCommand::Recover(Recovery::ResendImplementation {
        conclusion,
        delivery: shown.delivery.clone(),
        attempt: shown.attempt.clone(),
    }));

    assert_eq!(retried, Ok(()));
    let finished = harness.implementation_finished();
    assert_eq!(finished.state, DispatchState::Delivered);
    assert_eq!(finished.request, first.request);
    assert_eq!(harness.saved().implementations.len(), 1);
}
