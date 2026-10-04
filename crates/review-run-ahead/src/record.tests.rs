use super::*;

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
    }
}

#[test]
fn a_record_saved_without_the_later_fields_reads_back_as_a_running_fork() {
    let saved = serde_json::to_value(record()).unwrap();

    for field in ["turn", "exit", "usage", "discarded", "cleaned"] {
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
