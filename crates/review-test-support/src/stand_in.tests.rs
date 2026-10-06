use super::*;

fn fork(session: &str) -> StandInRole {
    StandInRole::Fork {
        session: session.into(),
    }
}

#[test]
fn each_stand_in_reports_its_events_in_order_then_its_end() {
    let directory = tempfile::tempdir().unwrap();
    let events = StandInEvents::listen(&directory.path().join("events.sock"));
    let path = directory.path().join("events.sock");
    let (agent, _) = StandInConnection::connect(&path, StandInRole::Agent).unwrap();

    agent.report(&StandInEvent::PromptReceived {
        text: "first".into(),
    });
    agent.report(&StandInEvent::TurnStarted);
    drop(agent);

    let reported = events.events_until("the agent's end", |reported| {
        reported.event == StandInEvent::Disconnected
    });
    let agent_events: Vec<_> = reported
        .into_iter()
        .inspect(|reported| assert_eq!(reported.from, StandInRole::Agent))
        .map(|reported| reported.event)
        .collect();
    assert_eq!(
        agent_events,
        [
            StandInEvent::Hello {
                role: StandInRole::Agent
            },
            StandInEvent::PromptReceived {
                text: "first".into()
            },
            StandInEvent::TurnStarted,
            StandInEvent::Disconnected,
        ]
    );
}

#[test]
fn a_command_reaches_the_stand_in_of_its_role_only() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("events.sock");
    let events = StandInEvents::listen(&path);
    let (_first, first_commands) = StandInConnection::connect(&path, fork("first")).unwrap();
    let (_second, second_commands) = StandInConnection::connect(&path, fork("second")).unwrap();

    events.send(&fork("second"), &StandInCommand::Submit);
    events.send(&fork("first"), &StandInCommand::End);

    assert_eq!(second_commands.recv().unwrap(), StandInCommand::Submit);
    assert_eq!(first_commands.recv().unwrap(), StandInCommand::End);
}

#[test]
fn a_marker_returns_what_the_agent_reported_before_it() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("events.sock");
    let events = StandInEvents::listen(&path);
    let (agent, _) = StandInConnection::connect(&path, StandInRole::Agent).unwrap();
    agent.report(&StandInEvent::TurnFinished);

    let before = events.mark(|prompt| {
        // The stand-in agent reads the marker as its next prompt.
        agent.report(&StandInEvent::Marker {
            id: marker_of(prompt).unwrap(),
        });
    });

    let before: Vec<_> = before.into_iter().map(|reported| reported.event).collect();
    assert_eq!(
        before,
        [
            StandInEvent::Hello {
                role: StandInRole::Agent
            },
            StandInEvent::TurnFinished,
            StandInEvent::Marker { id: 0 },
        ]
    );
    assert_eq!(marker_of("a prompt"), None);
}

#[test]
fn a_stand_in_that_connects_again_takes_the_commands_of_its_role() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("events.sock");
    let events = StandInEvents::listen(&path);
    let (first, _) = StandInConnection::connect(&path, StandInRole::Agent).unwrap();
    events.wait_for("the first agent", |reported| {
        matches!(reported.event, StandInEvent::Hello { .. })
    });
    let (_second, commands) = StandInConnection::connect(&path, StandInRole::Agent).unwrap();
    events.wait_for("the second agent", |reported| {
        matches!(reported.event, StandInEvent::Hello { .. })
    });

    drop(first);
    events.wait_for("the first agent's end", |reported| {
        reported.event == StandInEvent::Disconnected
    });
    events.send(&StandInRole::Agent, &StandInCommand::EndTurn);

    assert_eq!(commands.recv().unwrap(), StandInCommand::EndTurn);
}

#[test]
fn a_wait_on_every_event_sees_those_a_wait_in_turn_went_through() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("events.sock");
    let events = StandInEvents::listen(&path);
    let (agent, _) = StandInConnection::connect(&path, StandInRole::Agent).unwrap();
    agent.report(&StandInEvent::TurnStarted);
    agent.report(&StandInEvent::TurnFinished);
    events.wait_for("the end of the turn", |reported| {
        reported.event == StandInEvent::TurnFinished
    });
    agent.report(&StandInEvent::Exited);

    let received = events.wait_until("the agent's exit", |received| {
        received
            .iter()
            .any(|reported| reported.event == StandInEvent::Exited)
    });

    assert!(
        received
            .iter()
            .any(|reported| reported.event == StandInEvent::TurnStarted)
    );
    // A wait in turn goes on after the events the previous one went through.
    assert_eq!(
        events.wait_for("the exit", |_| true).event,
        StandInEvent::Exited
    );
}

#[test]
#[should_panic(expected = "a stand-in wrote")]
fn a_line_that_is_no_event_fails_the_test() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("events.sock");
    let events = StandInEvents::listen(&path);
    let mut stand_in = UnixStream::connect(&path).unwrap();

    stand_in.write_all(b"not json\n").unwrap();

    events.wait_for("anything", |_| true);
}
