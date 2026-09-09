use super::*;

fn start(text: &str) -> Post {
    Post::start(
        DiffRangeAnchor {
            source_checkpoint: "initial".into(),
            old_path: None,
            new_path: Some("gone.rs".into()),
            old_lines: None,
            new_lines: Some(0..1),
            target_kind: review_guide::GuideAnchorKind::Lines,
            source_hunk_count: 1,
            old_content: None,
            new_content: Some(b"original\n".to_vec()),
            diff_hash: "hash".into(),
        },
        "+original".into(),
        text.into(),
    )
}

fn agent_reply(thread: &ThreadId, text: &str) -> Post {
    Post::agent_reply(
        thread.clone(),
        MessageId::parse(&uuid::Uuid::new_v4().to_string()).unwrap(),
        text.into(),
    )
}

#[test]
fn updates_include_full_history_only_for_threads_with_new_comments() {
    let mut book = ReviewThreads::new("change".into());
    let first = start("Explain this");
    let thread = first.thread_id().clone();
    book.post(first).unwrap();
    let unrelated = start("Other question");
    let other_thread = unrelated.thread_id().clone();
    book.post(unrelated).unwrap();
    let through = book.sequence();
    book.mark_retrieved("agent", &thread, through);
    book.mark_retrieved("agent", &other_thread, through);
    book.post(agent_reply(&thread, "First answer")).unwrap();
    assert!(!book.has_new_messages("agent"));

    book.post(Post::reply(thread.clone(), "And this part?".into()))
        .unwrap();
    let updated = book.new_messages("agent");
    assert_eq!(updated.len(), 1);
    assert_eq!(updated[0].id, thread);
    assert_eq!(updated[0].excerpt, "+original");
    assert_eq!(
        updated[0]
            .messages
            .iter()
            .map(|message| message.text.as_str())
            .collect::<Vec<_>>(),
        ["Explain this", "First answer", "And this part?"]
    );
    assert_eq!(book.new_messages("another-agent").len(), 2);
}

#[test]
fn a_read_snapshot_does_not_hide_a_concurrent_post() {
    let mut book = ReviewThreads::new("change".into());
    let first = start("First");
    let thread = first.thread_id().clone();
    book.post(first).unwrap();
    let fetched = book.sequence();
    book.post(Post::reply(thread.clone(), "Arrived during read".into()))
        .unwrap();
    book.mark_retrieved("agent", &thread, fetched);
    assert!(book.has_new_messages("agent"));
    book.mark_retrieved("agent", &thread, book.sequence());
    assert!(!book.has_new_messages("agent"));
    book.retry("agent", &thread).unwrap();
    assert_eq!(book.new_messages("agent")[0].messages.len(), 2);
}

#[test]
fn retries_do_not_duplicate_or_overwrite_messages() {
    let mut book = ReviewThreads::new("change".into());
    let first = start("First");
    let thread = first.thread_id().clone();
    let id = book.post(first.clone()).unwrap();
    assert_eq!(book.post(first).unwrap(), id);
    let reply = agent_reply(&thread, "Answer");
    book.post(reply.clone()).unwrap();
    book.post(reply.clone()).unwrap();
    let conflicting = Post::agent_reply(
        thread.clone(),
        reply.message().id.clone(),
        "Different".into(),
    );
    assert!(book.post(conflicting).is_err());
    assert_eq!(book.thread(&thread).unwrap().messages.len(), 2);
    assert_eq!(book.sequence(), 2);
}

#[test]
fn unknown_threads_reject_replies_instead_of_creating_new_threads() {
    let mut book = ReviewThreads::new("change".into());
    let first = start("First");
    let thread = first.thread_id().clone();
    assert!(book.post(agent_reply(&thread, "Late")).is_err());
    assert!(book.threads().is_empty());
    assert!(book.post(start(" \n")).is_err());
}

#[test]
fn resolution_and_reviewer_reads_are_independent_of_agent_retrieval() {
    let mut book = ReviewThreads::new("change".into());
    let post = start("Question");
    let thread = post.thread_id().clone();
    book.post(post).unwrap();
    book.post(agent_reply(&thread, "First answer")).unwrap();
    let displayed = book.sequence();
    book.set_resolution(&thread, Resolution::Resolved).unwrap();
    book.mark_retrieved("agent", &thread, displayed);
    assert!(book.thread(&thread).unwrap().has_unread_replies());
    book.post(agent_reply(&thread, "Arrived after the display"))
        .unwrap();
    book.mark_read(&thread, displayed).unwrap();
    assert_eq!(
        book.counts(),
        ThreadCounts {
            total: 1,
            open: 0,
            unread: 1,
            waiting: 0
        }
    );
    book.mark_read(&thread, book.sequence()).unwrap();
    assert!(!book.thread(&thread).unwrap().has_unread_replies());
    assert_eq!(
        book.thread(&thread).unwrap().resolution,
        Resolution::Resolved
    );
    assert!(!book.has_new_messages("agent"));
    book.set_resolution(&thread, Resolution::Open).unwrap();
    assert_eq!(book.counts().open, 1);
}

#[test]
fn histories_without_attention_metadata_default_to_open_and_unread() {
    let mut book = ReviewThreads::new("change".into());
    let post = start("Question");
    let thread = post.thread_id().clone();
    book.post(post).unwrap();
    book.post(agent_reply(&thread, "Answer")).unwrap();
    let mut stored = serde_json::to_value(&book).unwrap();
    let row = stored["threads"][0].as_object_mut().unwrap();
    row.remove("resolution");
    row.remove("seen_reply_through");
    row.remove("seen_replies");
    let restored: ReviewThreads = serde_json::from_value(stored).unwrap();
    assert_eq!(restored.counts().open, 1);
    assert_eq!(restored.counts().unread, 1);
}

#[test]
fn reading_replies_out_of_order_preserves_unseen_and_later_answers() {
    let mut book = ReviewThreads::new("change".into());
    let question = start("Question");
    let thread = question.thread_id().clone();
    let question_id = book.post(question).unwrap();
    let first = book
        .post(agent_reply(&thread, "Earlier unseen answer"))
        .unwrap();
    let second = book.post(agent_reply(&thread, "Visible answer")).unwrap();
    book.mark_replies_read(&[second.clone(), question_id]);
    let mut book: ReviewThreads =
        serde_json::from_str(&serde_json::to_string(&book).unwrap()).unwrap();
    assert!(book.thread(&thread).unwrap().has_unread_reply(&first));
    assert!(!book.thread(&thread).unwrap().has_unread_reply(&second));
    assert!(book.has_new_messages("agent"));
    let late = book.post(agent_reply(&thread, "Late answer")).unwrap();
    book.mark_replies_read(&[first, second]);
    assert!(book.thread(&thread).unwrap().has_unread_reply(&late));
    assert_eq!(book.counts().unread, 1);
    book.mark_replies_read(&[late]);
    assert_eq!(book.counts().unread, 0);
    assert_eq!(book.counts().open, 1);
}
