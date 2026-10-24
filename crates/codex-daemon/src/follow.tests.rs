use super::*;

fn snapshot(session: &str) -> Incoming {
    Incoming::Notice {
        session: session.to_owned(),
        notice: Notice::GoalSnapshot,
    }
}

#[test]
fn the_snapshot_of_the_client_s_own_join_is_no_resume() {
    let mut hearing = Hearing::joining("t1");

    assert_eq!(hearing.hear(snapshot("t1")), None);
    assert_eq!(hearing.hear(snapshot("t1")), Some(SessionEvent::Resumed));
    assert_eq!(hearing.hear(snapshot("t1")), Some(SessionEvent::Resumed));
}

#[test]
fn what_the_daemon_says_of_another_session_is_not_reported() {
    let mut hearing = Hearing::joining("t1");
    hearing.hear(snapshot("t1"));

    assert_eq!(hearing.hear(snapshot("t2")), None);
    assert_eq!(
        hearing.hear(Incoming::Notice {
            session: "t2".to_owned(),
            notice: Notice::Event(SessionEvent::TurnStarted {
                turn: "u1".to_owned()
            }),
        }),
        None
    );
}

#[test]
fn only_the_join_s_answer_says_the_client_joined() {
    let mut hearing = Hearing::joining("t1");

    assert_eq!(
        hearing.hear(Incoming::Answer {
            id: wire::INITIALIZE,
            result: Ok(None),
        }),
        None
    );
    assert_eq!(
        hearing.hear(Incoming::Answer {
            id: wire::JOIN,
            result: Ok(Some(SessionStatus::Idle)),
        }),
        Some(SessionEvent::Joined(Some(SessionStatus::Idle)))
    );
}

fn lost(error: &str) -> Ended {
    Ended::Lost(error.to_owned())
}

#[test]
fn a_refused_first_join_ends_the_follow() {
    let mut progress = Progress::default();

    assert_eq!(
        progress.ended(Ended::Refused("no rollout found".to_owned())),
        Next::Stop(SessionEvent::Refused("no rollout found".to_owned()))
    );
}

#[test]
fn once_joined_a_refusal_is_a_loss_the_client_retries() {
    let mut progress = Progress::default();
    progress.joined();

    assert_eq!(
        progress.ended(Ended::Refused("loading".to_owned())),
        Next::Retry(Some(SessionEvent::Lost(
            "the Codex daemon refused to join the session again: loading".to_owned()
        )))
    );
    assert_eq!(
        progress.ended(Ended::Refused("loading".to_owned())),
        Next::Retry(None)
    );
}

#[test]
fn a_loss_is_reported_once_until_the_client_joins_again() {
    let mut progress = Progress::default();

    assert_eq!(
        progress.ended(lost("gone")),
        Next::Retry(Some(SessionEvent::Lost("gone".to_owned())))
    );
    assert_eq!(progress.ended(lost("still gone")), Next::Retry(None));
    progress.joined();
    assert_eq!(
        progress.ended(lost("gone again")),
        Next::Retry(Some(SessionEvent::Lost("gone again".to_owned())))
    );
}
