use super::*;

fn from(role: StandInRole, event: StandInEvent) -> Reported {
    Reported { from: role, event }
}

fn fork(session: &str) -> StandInRole {
    StandInRole::Fork {
        session: session.into(),
    }
}

fn started(session: &str, choice: &str) -> Reported {
    from(
        fork(session),
        StandInEvent::ForkStarted {
            parent: "agent".into(),
            prompt: format!("Explore round: r\nSelected option ID: {choice}\n"),
        },
    )
}

#[test]
fn each_fork_shows_its_choice_and_how_far_it_got() {
    let received = [
        from(fork("a"), StandInEvent::Hello { role: fork("a") }),
        started("a", "keep"),
        started("b", "change"),
        from(fork("a"), StandInEvent::ForkSubmitted),
        from(fork("b"), StandInEvent::ForkFinished),
        from(fork("b"), StandInEvent::Disconnected),
    ];

    let forks = forks(&received);

    let summary: Vec<_> = forks
        .iter()
        .map(|fork| {
            (
                fork.session.as_str(),
                fork.choice.as_deref(),
                fork.submitted,
                fork.finished,
                fork.exited,
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            ("a", Some("keep"), true, false, false),
            ("b", Some("change"), false, true, true),
        ]
    );
    assert_eq!(forks[0].parent, "agent");
}

#[test]
fn the_agent_s_session_is_the_one_it_reported_last() {
    let received = [
        from(
            StandInRole::Agent,
            StandInEvent::SessionReported {
                session: "own".into(),
                start: "startup".into(),
            },
        ),
        from(
            StandInRole::Agent,
            StandInEvent::ResumeReceived {
                session: "fork".into(),
            },
        ),
        from(
            StandInRole::Agent,
            StandInEvent::SessionReported {
                session: "fork".into(),
                start: "resume".into(),
            },
        ),
        // A fork's own session is no session of the agent's.
        from(
            fork("other"),
            StandInEvent::SessionReported {
                session: "other".into(),
                start: "startup".into(),
            },
        ),
        from(
            fork("other"),
            StandInEvent::ResumeReceived {
                session: "other".into(),
            },
        ),
    ];

    assert_eq!(
        agent_sessions(&received),
        AgentSessions {
            session: Some("fork".into()),
            resumed: vec!["fork".into()],
        }
    );
}
