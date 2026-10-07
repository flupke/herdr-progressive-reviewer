use super::*;
use crate::PlainReason;

fn record() -> ForkRecord {
    ForkRecord {
        question: "cache-eviction".into(),
        version: 1,
        choice: "keep".into(),
        answer: Some("fork-answer".into()),
        session: "fork-session".into(),
        from: Some("agent-session".into()),
        transcripts: "/state/projects".into(),
        reviewer: ProcessStamp { pid: 1, started: 2 },
        process: Some(ProcessStamp { pid: 3, started: 4 }),
        taken_at_ms: 5,
        turn: None,
        exit: None,
        usage: None,
        discarded: None,
        cleaned: false,
        continued: None,
    }
}

#[test]
fn a_record_saved_without_the_later_fields_reads_back_as_a_running_fork() {
    let mut fork = record();
    fork.answer = None;
    fork.from = None;
    let saved = serde_json::to_value(&fork).unwrap();

    for field in [
        "answer",
        "from",
        "turn",
        "exit",
        "usage",
        "discarded",
        "cleaned",
        "continued",
    ] {
        assert!(saved.get(field).is_none(), "{field}: {saved}");
    }
    assert_eq!(serde_json::from_value::<ForkRecord>(saved).unwrap(), fork);
}

#[test]
fn a_discarded_fork_keeps_its_first_reason() {
    let mut fork = record();

    fork.discard(DiscardReason::Answered, 10);
    fork.discard(DiscardReason::Reset, 20);

    assert_eq!(
        fork.discarded,
        Some(Discard {
            reason: DiscardReason::Answered,
            prepared: false,
            at_ms: 10,
        })
    );
}

#[test]
fn a_fork_whose_session_the_pane_agent_may_run_is_never_discarded() {
    for (continued, kept) in [
        (Continuation::Switching { at_ms: 1 }, true),
        (Continuation::Switched { at_ms: 1 }, true),
        (
            Continuation::Failed {
                at_ms: 1,
                error: "Herdr did not report it".into(),
                typed: true,
            },
            true,
        ),
        (
            Continuation::Failed {
                at_ms: 1,
                error: "the agent is working".into(),
                typed: false,
            },
            false,
        ),
        (
            Continuation::Undone {
                at_ms: 1,
                reason: "the reviewer stopped waiting".into(),
            },
            false,
        ),
    ] {
        let mut fork = record();
        fork.continued = Some(continued);

        fork.discard(DiscardReason::ReviewerStopped, 10);

        assert_eq!(fork.discarded.is_none(), kept, "{:?}", fork.continued);
    }
}

#[test]
fn a_turn_is_prepared_once_the_pane_s_agent_runs_its_fork_s_session_and_every_plain_chain_has_its_reason()
 {
    let answer = |request: &str, path: TurnPath| AnswerRecord {
        question: "cache-eviction".into(),
        version: 1,
        answer: format!("answer-{request}"),
        request: request.into(),
        at_ms: 5,
        path,
    };
    let mut switched = record();
    switched.continued = Some(Continuation::Switched { at_ms: 6 });
    let mut switching = record();
    switching.session = "switching".into();
    switching.continued = Some(Continuation::Switching { at_ms: 6 });
    let forks = RoundForks {
        forks: vec![switched, switching],
        answers: vec![
            answer(
                "r1",
                TurnPath::Prepared {
                    session: "fork-session".into(),
                },
            ),
            answer(
                "r2",
                TurnPath::Plain {
                    reason: PlainReason::Comment,
                },
            ),
            answer(
                "r3",
                TurnPath::Prepared {
                    session: "switching".into(),
                },
            ),
        ],
        ..RoundForks::default()
    };

    let paths: Vec<_> = forks
        .turn_paths()
        .map(|(request, path)| (request, path.clone()))
        .collect();
    assert_eq!(
        paths,
        [
            (
                "r1",
                TurnPath::Prepared {
                    session: "fork-session".into()
                }
            ),
            (
                "r2",
                TurnPath::Plain {
                    reason: PlainReason::Comment
                }
            ),
        ]
    );
    assert_eq!(forks.continued_as("switching"), Some("r3"));
    assert_eq!(forks.continued_as("other"), None);
}

#[test]
fn run_ahead_stops_for_the_round_once_enough_forks_failed_in_a_row() {
    let mut forks = RoundForks::default();

    for _ in 1..FAILURES_TO_HALT {
        assert!(!forks.fork_failed(1));
    }
    forks.fork_kept_a_turn();
    for _ in 1..FAILURES_TO_HALT {
        assert!(!forks.fork_failed(2));
    }
    assert_eq!(
        forks.halted_at_ms, None,
        "a kept turn starts the count again"
    );
    assert!(forks.fork_failed(3), "this failure stops run-ahead");
    assert!(!forks.fork_failed(4), "it stops once");
    assert_eq!(forks.halted_at_ms, Some(3));
}

#[test]
fn a_switch_that_runs_or_failed_after_the_resume_was_typed_is_unsettled() {
    for (continued, unsettled) in [
        (None, false),
        (Some(Continuation::Switching { at_ms: 1 }), true),
        (Some(Continuation::Switched { at_ms: 1 }), false),
        (
            Some(Continuation::Failed {
                at_ms: 1,
                error: "Herdr did not report it".into(),
                typed: true,
            }),
            true,
        ),
        (
            Some(Continuation::Failed {
                at_ms: 1,
                error: "the agent is working".into(),
                typed: false,
            }),
            false,
        ),
        (
            Some(Continuation::Undone {
                at_ms: 1,
                reason: "the reviewer stopped waiting".into(),
            }),
            false,
        ),
    ] {
        let mut fork = record();
        fork.continued = continued;

        assert_eq!(fork.is_unsettled(), unsettled, "{:?}", fork.continued);
    }
}
