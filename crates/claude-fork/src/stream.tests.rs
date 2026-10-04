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

#[test]
fn a_submit_is_answered_once_its_tool_result_comes_or_the_fork_ends() {
    let mut tally = Box::new(StreamTally::new(Box::new(|_| {})));
    let answered = tally.submit_answered();
    for line in [
        r#"{"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"t1","name":"Read","input":{}}]}}"#,
        r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1"}]}}"#,
        r#"{"type":"assistant","message":{"id":"m2","content":[{"type":"tool_use","id":"t2","name":"mcp__herdr_reviewer__submit_question","input":{}}]}}"#,
    ] {
        tally.line(line);
    }
    assert!(
        !answered.wait(Duration::ZERO),
        "another tool's result, or a submit without its answer, is not the submit's answer"
    );

    tally.line(
        r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t2"}]}}"#,
    );

    assert!(answered.wait(Duration::ZERO));
    let ended = Box::new(StreamTally::new(Box::new(|_| {})));
    let gone = ended.submit_answered();
    ended.ended(Exit {
        status: "exit status: 0".into(),
        stderr: String::new(),
    });
    assert!(gone.wait(Duration::ZERO));
}
