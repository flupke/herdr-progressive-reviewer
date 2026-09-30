//! Every thread document version that earlier builds wrote still loads, and what this
//! build writes is byte for byte what the build before it wrote.

use std::sync::Arc;

use review_threads::{MessageId, Post, Resolution, ThreadId, ThreadSource};
use serde_json::json;

use super::*;
use crate::stored_fixtures::{CONTEXT, CONTEXT_KEY, THREADS_V4, inline_thread};

const THREAD: &str = "0b6f2a4e-1c3d-4e5f-8a9b-0c1d2e3f4a5b";

fn open(directory: &tempfile::TempDir) -> ReviewStore {
    ReviewStore::open(directory.path().join("state"), directory.path()).unwrap()
}

fn unit() -> ReviewUnit {
    "change".into()
}

fn thread_id() -> ThreadId {
    serde_json::from_value(json!(THREAD)).unwrap()
}

fn write_document(store: &ReviewStore, document: &Value) {
    store.write_json_fixture(&store.threads_fixture_path(), &document.to_string());
}

/// A version 2 or 3 document holding one thread with `messages`.
fn legacy_document(version: u8, messages: &Value, readers: &Value) -> Value {
    json!({"version": version, "conversations": {
        "review_unit": "change",
        "threads": [inline_thread(json!({"id": THREAD, "messages": messages}))],
        "sequence": messages.as_array().unwrap().len(),
        "readers": readers,
    }})
}

fn comment(id: &str, sequence: u64) -> Value {
    json!({"id": id, "author": "reviewer", "text": format!("Comment {sequence}"), "sequence": sequence})
}

fn legacy_reply(id: &str, sequence: u64) -> Value {
    json!({"id": id, "author": "agent", "text": format!("Reply {sequence}"), "sequence": sequence})
}

#[test]
fn a_version_4_document_loads_and_is_written_back_byte_for_byte() {
    let directory = tempfile::tempdir().unwrap();
    let store = open(&directory);
    store.write_context_fixture();
    let path = store.threads_fixture_path();
    store.write_json_fixture(&path, THREADS_V4);

    let threads = store.load_threads(&unit()).unwrap();
    let [answered, waiting] = threads.threads() else {
        panic!("the fixture holds two threads");
    };
    assert_eq!(answered.resolution, Resolution::Resolved);
    assert_eq!(answered.messages[0].text, "Why \"this\"?\nExplain.");
    assert!(!answered.has_unread_replies());
    assert!(waiting.is_waiting());
    assert_eq!(threads.new_messages().len(), 1);
    assert_eq!(
        *answered.source,
        serde_json::from_str::<ThreadSource>(CONTEXT).unwrap()
    );
    assert!(
        Arc::ptr_eq(&answered.source, &waiting.source),
        "threads with the same source share one loaded copy"
    );

    store.save_threads(&threads).unwrap();
    assert_eq!(ReviewStore::decoded_fixture(&path), THREADS_V4);
}

#[test]
fn a_version_3_document_loads_and_is_written_as_the_version_4_bytes_of_earlier_builds() {
    let directory = tempfile::tempdir().unwrap();
    let store = open(&directory);
    write_document(
        &store,
        &json!({"version": 3, "conversations": {
            "review_unit": "change",
            "recipient": {"pane_id": "retired-agent", "workspace_id": "old-workspace"},
            "threads": [
                inline_thread(json!({
                    "id": THREAD,
                    "messages": [
                        {"id": "11111111-1111-4111-8111-111111111111", "author": "reviewer", "text": "Why \"this\"?\nExplain.", "sequence": 1},
                        {"id": "22222222-2222-4222-8222-222222222222", "author": "agent", "text": "Because.", "in_reply_to": "11111111-1111-4111-8111-111111111111", "sequence": 2},
                    ],
                    "resolution": "resolved",
                    "seen_reply_through": 2,
                    "seen_replies": ["22222222-2222-4222-8222-222222222222"],
                })),
                inline_thread(json!({
                    "id": "3c4d5e6f-7a8b-4c9d-8e0f-1a2b3c4d5e6f",
                    "messages": [
                        {"id": "33333333-3333-4333-8333-333333333333", "author": "reviewer", "text": "Pending", "sequence": 3},
                    ],
                    "resolution": "open",
                    "seen_reply_through": 0,
                    "seen_replies": [],
                })),
            ],
            "sequence": 3,
            "answered": {THREAD: 1},
        }}),
    );

    let threads = store.load_threads(&unit()).unwrap();
    store.save_threads(&threads).unwrap();
    assert_eq!(
        ReviewStore::decoded_fixture(&store.threads_fixture_path()),
        THREADS_V4
    );
    assert_eq!(
        ReviewStore::decoded_fixture(&store.thread_source_path(CONTEXT_KEY).unwrap()),
        CONTEXT
    );
}

#[test]
fn version_3_answers_survive_a_recipient_handoff_without_consuming_later_comments() {
    let messages = json!([
        comment("11111111-1111-4111-8111-111111111111", 1),
        legacy_reply("22222222-2222-4222-8222-222222222222", 2),
        comment("33333333-3333-4333-8333-333333333333", 3),
    ]);
    for (readers, answered) in [
        (json!({"old-agent": {THREAD: 3}}), true),
        (json!({}), false),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let store = open(&directory);
        write_document(&store, &legacy_document(3, &messages, &readers));

        let threads = store.load_threads(&unit()).unwrap();
        assert!(threads.thread(&thread_id()).unwrap().is_waiting());
        assert_eq!(threads.has_new_messages(), !answered);
        if !answered {
            continue;
        }
        let ((), threads) = store
            .update_threads(&unit(), |book| {
                book.post(Post::reply(thread_id(), "Only unanswered work".into()))
                    .map(drop)
            })
            .unwrap();
        assert_eq!(threads.new_messages()[0].messages.len(), 4);
    }
}

#[test]
fn version_3_answers_are_recovered_from_reply_boundaries_when_a_retry_erased_the_cursor() {
    let answered = json!({"id": "22222222-2222-4222-8222-222222222222", "author": "agent", "text": "Done", "in_reply_to": "11111111-1111-4111-8111-111111111111", "sequence": 2});
    for (later_comment, pending) in [(false, false), (true, true)] {
        let mut messages = vec![
            comment("11111111-1111-4111-8111-111111111111", 1),
            answered.clone(),
        ];
        if later_comment {
            messages.push(comment("33333333-3333-4333-8333-333333333333", 3));
        }
        let directory = tempfile::tempdir().unwrap();
        let store = open(&directory);
        write_document(&store, &legacy_document(3, &json!(messages), &json!({})));

        let threads = store.load_threads(&unit()).unwrap();
        assert_eq!(threads.has_new_messages(), pending);
        if pending {
            assert_eq!(threads.new_messages()[0].messages.len(), 3);
        }
    }
}

#[test]
fn version_2_recovery_only_delivers_comments_still_waiting_for_an_answer() {
    for late_before_reply in [false, true] {
        let messages = if late_before_reply {
            json!([
                comment("11111111-1111-4111-8111-111111111111", 1),
                comment("33333333-3333-4333-8333-333333333333", 2),
                legacy_reply("22222222-2222-4222-8222-222222222222", 3),
            ])
        } else {
            json!([
                comment("11111111-1111-4111-8111-111111111111", 1),
                legacy_reply("22222222-2222-4222-8222-222222222222", 2),
                comment("33333333-3333-4333-8333-333333333333", 3),
            ])
        };
        let directory = tempfile::tempdir().unwrap();
        let store = open(&directory);
        // Version 2 advanced the cursor on every fetch, including the final one.
        write_document(
            &store,
            &legacy_document(2, &messages, &json!({"agent": {THREAD: 3}})),
        );

        let recovered = store.load_threads(&unit()).unwrap();
        let thread = recovered.thread(&thread_id()).unwrap();
        // Version 2 recorded no attention state: the thread is open and its reply unread.
        assert_eq!(recovered.counts().open, 1);
        assert_eq!(recovered.counts().unread, 1);
        assert_eq!(
            *thread.source,
            serde_json::from_str::<ThreadSource>(CONTEXT).unwrap()
        );
        assert_eq!(recovered.has_new_messages(), !late_before_reply);
        assert_eq!(
            recovered.new_messages().len(),
            usize::from(!late_before_reply)
        );
        if late_before_reply {
            // Without an exact boundary, legacy replies cover preceding comments, just as
            // the thread's visible Waiting status does.
            assert!(!thread.is_waiting());
            assert_eq!(recovered.pending_comment_sequence(), None);
            assert!(recovered.retry(&thread_id()).is_err());
            continue;
        }
        let comment = thread.last_comment().unwrap().id.clone();
        store
            .update_threads(&unit(), |book| {
                book.answer(Post::answer(
                    thread_id(),
                    MessageId::parse("b5c06df5-6b11-4134-9fb7-d18b4c310097").unwrap(),
                    "Recovered answer".into(),
                    comment,
                ))
            })
            .unwrap();
        let saved = store.load_threads(&unit()).unwrap();
        assert_eq!(saved.threads()[0].messages.len(), 4);
        assert!(!saved.has_new_messages());
        let written: Value =
            serde_json::from_str(&ReviewStore::decoded_fixture(&store.threads_fixture_path()))
                .unwrap();
        assert_eq!(written["version"], 4);
        assert_eq!(
            written["conversations"]["threads"][0]["context"],
            CONTEXT_KEY
        );
        assert!(written["conversations"].get("readers").is_none());
    }
}

#[test]
fn unsupported_versions_are_errors_and_remain_untouched() {
    for version in [1, 5] {
        let directory = tempfile::tempdir().unwrap();
        let store = open(&directory);
        let document = legacy_document(version, &json!([]), &json!({}));
        write_document(&store, &document);
        assert!(store.update_threads(&unit(), |_| Ok(())).is_err());
        assert_eq!(
            ReviewStore::decoded_fixture(&store.threads_fixture_path()),
            document.to_string()
        );
    }
}
