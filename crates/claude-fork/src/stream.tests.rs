use std::sync::mpsc;

use super::*;

#[test]
fn a_fork_costs_the_tokens_of_its_own_messages_counted_once_each() {
    let (sent, received) = mpsc::channel();
    let mut tally = Box::new(StreamTally::new(Box::new(move |end| {
        sent.send(end).unwrap();
    })));
    let usage = r#"{"input_tokens":2,"cache_creation_input_tokens":300,"cache_read_input_tokens":9000,"output_tokens":40}"#;
    for line in [
        r#"{"type":"system","subtype":"init","session_id":"fork"}"#.to_owned(),
        format!(r#"{{"type":"assistant","message":{{"id":"m1","usage":{usage}}}}}"#),
        format!(r#"{{"type":"assistant","message":{{"id":"m1","usage":{usage}}}}}"#),
        format!(r#"{{"type":"assistant","message":{{"id":"m2","usage":{usage}}}}}"#),
        "running 1 test".to_owned(),
    ] {
        tally.line(&line);
    }

    tally.ended(Exit {
        status: "signal: 15 (SIGTERM)".into(),
        stderr: String::new(),
    });

    let end = received.recv().unwrap();
    assert_eq!(
        end.usage,
        TokenUsage {
            input: 4,
            cache_creation: 600,
            cache_read: 18_000,
            output: 80,
        }
    );
    assert!(!end.finished);
    assert_eq!(end.exit, "signal: 15 (SIGTERM)");
}
