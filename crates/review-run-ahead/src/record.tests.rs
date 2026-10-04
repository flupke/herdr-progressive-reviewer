use super::*;
use crate::PlainReason;

fn record() -> ForkRecord {
    ForkRecord {
        question: "cache-eviction".into(),
        version: 1,
        choice: "keep".into(),
        session: "fork-session".into(),
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
    let saved = serde_json::to_value(record()).unwrap();

    for field in ["turn", "exit", "usage", "discarded", "cleaned", "continued"] {
        assert!(saved.get(field).is_none(), "{field}: {saved}");
    }
    assert_eq!(
        serde_json::from_value::<ForkRecord>(saved).unwrap(),
        record()
    );
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
}
