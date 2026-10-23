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

/// A switch that failed for `error`.
fn switch_failed(error: &str) -> Failure {
    Failure::SwitchFailed {
        error: error.into(),
    }
}

#[test]
fn run_ahead_stops_for_the_round_once_it_failed_enough_times_in_a_row() {
    let mut forks = RoundForks::default();
    let failures = [
        Failure::Fork {
            exit: "exit status: 1".into(),
        },
        Failure::Unchecked {
            error: "the agent's pane is gone".into(),
        },
        Failure::NoForks {
            why: "unknown option --frobnicate".into(),
        },
    ];

    for failure in failures.iter().cycle().take(FAILURES_TO_HALT as usize - 1) {
        assert!(!forks.failed(failure.clone(), 1));
    }
    forks.prepared_turn_used();
    for failure in failures.iter().cycle().take(FAILURES_TO_HALT as usize - 1) {
        assert!(!forks.failed(failure.clone(), 2));
    }
    assert_eq!(
        forks.halt_notice(),
        None,
        "a prepared turn used starts the count again"
    );
    assert!(
        forks.failed(switch_failed("Herdr did not report it"), 3),
        "this failure stops run-ahead, whatever its kind"
    );
    assert!(
        !forks.failed(switch_failed("the agent is working"), 4),
        "it stops once"
    );
    assert_eq!(forks.halted_at_ms, Some(3));
    let notice = forks.halt_notice().unwrap();
    assert!(
        notice.contains("Herdr did not report it"),
        "the notice names the failure that stopped it: {notice}"
    );
}

#[test]
fn a_halt_saved_before_its_cause_was_kept_still_refuses_forks() {
    let saved: RoundForks = serde_json::from_value(serde_json::json!({
        "forks": [],
        "failed_in_a_row": 3,
        "halted_at_ms": 7,
    }))
    .unwrap();

    assert!(saved.halt_notice().is_some());
}

#[test]
fn a_halt_saves_its_cause() {
    let mut forks = RoundForks::default();
    for _ in 0..FAILURES_TO_HALT {
        forks.failed(switch_failed("Herdr did not report it"), 7);
    }

    assert_eq!(
        serde_json::to_value(&forks).unwrap(),
        serde_json::json!({
            "forks": [],
            "failed_in_a_row": FAILURES_TO_HALT,
            "halted_at_ms": 7,
            "halted_by": {"failure": "switch_failed", "error": "Herdr did not report it"},
        })
    );
}

#[test]
fn only_a_check_that_could_not_be_made_and_a_failed_switch_are_failures_of_an_answer() {
    assert_eq!(
        Failure::of_plain(&PlainReason::Unchecked {
            error: "Herdr is down".into()
        }),
        Some(Failure::Unchecked {
            error: "Herdr is down".into()
        })
    );
    assert_eq!(
        Failure::of_plain(&PlainReason::SwitchFailed {
            error: "Herdr did not report it".into()
        }),
        Some(switch_failed("Herdr did not report it"))
    );
    for reason in [
        PlainReason::Comment,
        PlainReason::NoForks { why: None },
        PlainReason::NoTurn,
        PlainReason::Withdrawn,
    ] {
        assert_eq!(Failure::of_plain(&reason), None, "{reason:?}");
    }
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
