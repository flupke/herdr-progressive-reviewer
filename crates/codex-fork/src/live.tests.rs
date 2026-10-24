use std::sync::mpsc;

use serde_json::json;

use super::*;

#[test]
fn a_turn_s_end_carries_the_tokens_of_its_requests_with_those_read_from_the_cache_apart() {
    let watch = TurnWatch::default();
    watch.forked("fork-1");
    let (ended, end) = mpsc::channel();
    watch.starting(Box::new(move |fork_end| ended.send(fork_end).unwrap()));
    watch.started("turn-1");
    for last in [
        json!({"inputTokens": 100, "cachedInputTokens": 90, "outputTokens": 5,
            "reasoningOutputTokens": 0, "totalTokens": 105}),
        json!({"inputTokens": 120, "cachedInputTokens": 100, "cacheWriteInputTokens": 15,
            "outputTokens": 7, "reasoningOutputTokens": 0, "totalTokens": 127}),
    ] {
        watch.hear(
            "thread/tokenUsage/updated",
            &json!({"threadId": "fork-1", "turnId": "turn-1",
                "tokenUsage": {"last": last, "total": {}}}),
        );
    }

    // Another turn's end is not this one's.
    watch.hear(
        "turn/completed",
        &json!({"threadId": "fork-1", "turn": {"id": "turn-0", "status": "completed"}}),
    );
    assert!(!watch.wait_over(Duration::ZERO));
    watch.hear(
        "turn/completed",
        &json!({"threadId": "fork-1", "turn": {"id": "turn-1", "status": "completed"}}),
    );

    assert!(watch.wait_over(Duration::ZERO));
    assert_eq!(
        end.try_recv().unwrap(),
        ForkEnd {
            exit: "its turn completed".into(),
            usage: TokenUsage {
                input: 15,
                cache_creation: 15,
                cache_read: 190,
                output: 12,
            },
            finished: true,
        }
    );
}

#[test]
fn an_interrupted_turn_did_not_finish() {
    let watch = TurnWatch::default();
    watch.forked("fork-1");
    let (ended, end) = mpsc::channel();
    watch.starting(Box::new(move |fork_end| ended.send(fork_end).unwrap()));

    watch.hear(
        "turn/completed",
        &json!({"threadId": "fork-1", "turn": {"id": "turn-1", "status": "interrupted"}}),
    );

    let end = end.try_recv().unwrap();
    assert!(!end.finished);
    assert_eq!(end.exit, "its turn was interrupted");
}
