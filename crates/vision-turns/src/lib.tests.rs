use serde_json::{Value, json};

use super::TurnLog;

fn turn(directory: &std::path::Path, number: u64) -> Value {
    let text = std::fs::read_to_string(directory.join(format!("turn-{number:06}.json"))).unwrap();
    serde_json::from_str(&text).unwrap()
}

#[test]
fn two_writers_number_their_turns_after_each_other() {
    let directory = tempfile::tempdir().unwrap();
    let explore = TurnLog::open(directory.path().to_owned()).unwrap();
    let threads = explore.clone();

    explore.record(&json!({"kind": "kickoff"}), None);
    threads.record(&json!({"kind": "comments"}), Some("not started".to_owned()));

    assert_eq!(
        turn(directory.path(), 1),
        json!({"turn": 1, "delivered": true, "kind": "kickoff"})
    );
    assert_eq!(
        turn(directory.path(), 2),
        json!({"turn": 2, "delivered": false, "error": "not started", "kind": "comments"})
    );
}

#[test]
fn a_reopened_log_continues_after_the_last_turn() {
    let directory = tempfile::tempdir().unwrap();
    TurnLog::open(directory.path().to_owned())
        .unwrap()
        .record(&json!({"kind": "kickoff"}), None);

    TurnLog::open(directory.path().to_owned())
        .unwrap()
        .record(&json!({"kind": "wakeup"}), None);

    assert_eq!(turn(directory.path(), 2)["kind"], "wakeup");
}
