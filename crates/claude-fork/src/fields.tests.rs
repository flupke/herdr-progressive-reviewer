use super::*;

#[test]
fn the_host_finds_the_transcripts_and_the_process_of_a_record_saved_by_an_earlier_version() {
    let saved = serde_json::json!({
        "question": "cache-eviction",
        "version": 1,
        "choice": "keep",
        "session": "fork-session",
        "transcripts": "/state/projects",
        "reviewer": {"pid": 1, "started": 2},
        "process": {"pid": 3, "started": 4},
        "taken_at_ms": 5,
    });

    let record: review_run_ahead::ForkRecord = serde_json::from_value(saved).unwrap();

    assert_eq!(
        transcripts(&record.host),
        Some(PathBuf::from("/state/projects"))
    );
    assert_eq!(
        process(&record.host),
        Some(ProcessStamp { pid: 3, started: 4 })
    );
}
