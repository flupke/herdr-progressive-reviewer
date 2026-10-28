use std::collections::BTreeMap;
use std::sync::Arc;

use review_source::DiffRangeAnchor;

use crate::{
    Draft, MessageId, Post, Recipient, ReviewThreads, SavedDrafts, ThreadId, ThreadSource,
};

fn anchor() -> DiffRangeAnchor {
    DiffRangeAnchor {
        source_checkpoint: "initial".into(),
        old_path: None,
        new_path: Some("a.rs".into()),
        old_lines: None,
        new_lines: Some(0..1),
        target_kind: review_source::AnchorKind::Lines,
        source_hunk_count: 1,
        old_content: None,
        new_content: Some(b"original\n".to_vec()),
        diff_hash: "hash".into(),
    }
}

fn start(book: &mut ReviewThreads, text: &str) -> ThreadId {
    let post = Post::start(anchor(), "+original".into(), text.into());
    let thread = post.thread_id().clone();
    book.post(post).unwrap();
    thread
}

fn answer(book: &mut ReviewThreads, thread: &ThreadId, text: &str) -> MessageId {
    let comment = book
        .thread(thread)
        .unwrap()
        .last_comment()
        .unwrap()
        .id
        .clone();
    book.answer(Post::answer(
        thread.clone(),
        MessageId::parse(&uuid::Uuid::new_v4().to_string()).unwrap(),
        text.into(),
        comment,
    ))
    .unwrap()
}

fn texts(book: &ReviewThreads, thread: &ThreadId) -> Vec<(String, u64)> {
    book.thread(thread)
        .unwrap()
        .messages
        .iter()
        .map(|message| (message.text.clone(), message.sequence()))
        .collect()
}

#[test]
fn threads_of_two_workspaces_keep_their_answers_and_pending_comments() {
    let mut main = ReviewThreads::new("change".into());
    let answered = start(&mut main, "Answered in main");
    answer(&mut main, &answered, "Main's answer");
    let mut task = ReviewThreads::new("change".into());
    let pending = start(&mut task, "Pending in the task");
    let read = start(&mut task, "Read in the task");
    let reply = answer(&mut task, &read, "Task's answer");
    task.mark_read(&read, task.sequence()).unwrap();

    main.merge(task.clone()).unwrap();

    assert_eq!(main.threads().len(), 3);
    assert_eq!(texts(&main, &pending), [("Pending in the task".into(), 3)]);
    assert_eq!(
        texts(&main, &read),
        [("Read in the task".into(), 4), ("Task's answer".into(), 5)]
    );
    assert_eq!(main.sequence(), 5);
    let waiting = main
        .new_messages()
        .into_iter()
        .map(|thread| thread.id)
        .collect::<Vec<_>>();
    assert_eq!(waiting, [pending]);
    assert!(!main.thread(&read).unwrap().has_unread_reply(&reply));
    let merged = main.clone();
    main.merge(task).unwrap();
    assert_eq!(main, merged);
}

#[test]
fn a_thread_continued_in_another_workspace_gains_only_what_it_lacks() {
    let mut main = ReviewThreads::new("change".into());
    let thread = start(&mut main, "First comment");
    let mut task = main.clone();
    answer(&mut task, &thread, "Answer in the task");
    main.post(Post::reply(thread.clone(), "Follow-up in main".into()))
        .unwrap();

    main.merge(task).unwrap();

    assert_eq!(
        texts(&main, &thread),
        [
            ("First comment".into(), 1),
            ("Follow-up in main".into(), 2),
            ("Answer in the task".into(), 3),
        ]
    );
    // The task's answer covers the first comment only: the follow-up still waits.
    assert_eq!(main.new_messages().len(), 1);
    assert_eq!(main.pending_comment_sequence(), Some(2));
}

#[test]
fn threads_of_another_review_are_refused() {
    let mut main = ReviewThreads::new("change".into());
    let mut other = ReviewThreads::new("other".into());
    start(&mut other, "Elsewhere");

    assert!(main.merge(other).is_err());
    assert!(main.threads().is_empty());
}

#[test]
fn a_tab_gets_the_pending_comments_it_posted_and_those_of_threads_retried_there() {
    let mut book = ReviewThreads::new("change".into());
    let post =
        Post::start(anchor(), "+original".into(), "From tab one".into()).from_tab("w1:t1".into());
    let thread = post.thread_id().clone();
    book.post(post).unwrap();
    let older = start(&mut book, "From an earlier build");
    let none = BTreeMap::new();
    let older_comment = book.thread(&older).unwrap().messages[0].sequence();
    let retried = BTreeMap::from([(older.clone(), older_comment)]);
    let tab = |tab, retried| Recipient { tab, retried };
    let ids = |threads: Vec<crate::ReviewThread>| {
        threads
            .into_iter()
            .map(|thread| thread.id)
            .collect::<Vec<_>>()
    };

    assert_eq!(
        ids(book.new_messages_for(&tab("w1:t1", &none))),
        std::slice::from_ref(&thread)
    );
    assert!(!book.has_new_messages_for(&tab("w1:t2", &none)));
    assert_eq!(
        ids(book.new_messages_for(&tab("w1:t2", &retried))),
        std::slice::from_ref(&older)
    );
    assert_eq!(
        book.pending_comment_sequence_for(&tab("w1:t2", &retried)),
        Some(2)
    );
    answer(&mut book, &thread, "Answered");
    assert!(!book.has_new_messages_for(&tab("w1:t1", &none)));
    assert!(book.has_new_messages_for(&tab("w1:t1", &retried)));
    // A Retry covers the comments posted before it: a later one waits for its own tab.
    answer(&mut book, &older, "Answered after the Retry");
    book.post(Post::reply(older.clone(), "Later, from tab three".into()).from_tab("w1:t3".into()))
        .unwrap();
    assert!(!book.has_new_messages_for(&tab("w1:t2", &retried)));
    assert!(book.has_new_messages_for(&tab("w1:t3", &none)));
}

#[test]
fn a_thread_keeps_its_draft_and_gains_the_drafts_of_other_threads() {
    let source = Arc::new(ThreadSource {
        anchor: anchor(),
        excerpt: "+original".into(),
    });
    let mut kept = Draft::start("a.rs".into(), source.clone());
    kept.text = "Kept".into();
    let mut replaced = kept.clone();
    replaced.text = "Same thread elsewhere".into();
    let mut added = Draft::start("a.rs".into(), source);
    added.text = "Added".into();
    let mut drafts = SavedDrafts::default();
    drafts.keep(kept);
    let mut other = SavedDrafts::default();
    other.keep(replaced);
    other.keep(added);

    drafts.merge(other);

    let texts = drafts
        .drafts()
        .iter()
        .map(|draft| draft.text.as_str())
        .collect::<Vec<_>>();
    assert_eq!(texts, ["Kept", "Added"]);
}
