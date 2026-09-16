use super::*;
use review_guide::DiffRangeAnchor;

#[test]
fn saved_recipient_fields_do_not_change_history_or_pending_work() {
    let mut book = ReviewThreads::new("change".into());
    let post = start("Completed question");
    let thread = post.thread_id().clone();
    let question = book.post(post).unwrap();
    book.answer(Post::answer(
        thread.clone(),
        MessageId::parse(&uuid::Uuid::new_v4().to_string()).unwrap(),
        "Completed answer".into(),
        question,
    ))
    .unwrap();
    let mut draft = crate::Draft::reply(
        book.thread(&thread).unwrap(),
        book.thread(&thread).unwrap().messages[0].id.clone(),
    );
    draft.text = "Private text".into();
    book.save_draft(draft).unwrap();
    let mut saved = serde_json::to_value(&book).unwrap();
    saved["recipient"] =
        serde_json::json!({"pane_id": "retired-agent", "workspace_id": "old-workspace"});
    let restored: ReviewThreads = serde_json::from_value(saved).unwrap();
    assert_eq!(restored, book);
    assert!(restored.new_messages().is_empty());
    assert!(
        serde_json::to_value(restored)
            .unwrap()
            .get("recipient")
            .is_none()
    );
}

#[test]
fn resolving_stops_pending_delivery_and_late_answers_do_not_reopen_it() {
    let mut book = ReviewThreads::new("change".into());
    let post = start("Question");
    let thread = post.thread_id().clone();
    let question = book.post(post).unwrap();
    book.set_resolution(&thread, Resolution::Resolved).unwrap();
    assert!(book.new_messages().is_empty());
    assert!(book.retry(&thread).is_err());
    book.set_resolution(&thread, Resolution::Open).unwrap();
    assert_eq!(book.new_messages().len(), 1);
    book.set_resolution(&thread, Resolution::Resolved).unwrap();
    book.answer(Post::answer(
        thread.clone(),
        MessageId::parse(&uuid::Uuid::new_v4().to_string()).unwrap(),
        "Late answer".into(),
        question,
    ))
    .unwrap();
    assert_eq!(
        book.thread(&thread).unwrap().resolution,
        Resolution::Resolved
    );
    assert_eq!(book.thread(&thread).unwrap().messages.len(), 2);
    book.set_resolution(&thread, Resolution::Open).unwrap();
    assert!(book.new_messages().is_empty());
    book.post(Post::reply(thread, "New follow-up".into()))
        .unwrap();
    assert_eq!(book.new_messages()[0].messages.len(), 3);
}

#[test]
fn legacy_answers_survive_handoff_without_consuming_later_comments() {
    let mut book = ReviewThreads::new("change".into());
    let post = start("Question");
    let thread = post.thread_id().clone();
    book.post(post).unwrap();
    answer(&mut book, &thread, "Done");
    let mut json = serde_json::to_value(&book).unwrap();
    json["readers"] = serde_json::json!({"old-agent": json["answered"].clone()});
    json.as_object_mut().unwrap().remove("answered");
    let mut migrated: ReviewThreads = serde_json::from_value(json).unwrap();
    migrated.migrate_answered_positions();
    assert!(migrated.new_messages().is_empty());
    migrated
        .post(Post::reply(thread, "Only unanswered work".into()))
        .unwrap();
    assert_eq!(migrated.new_messages()[0].messages.len(), 3);
}

#[test]
fn migration_recovers_exact_answers_after_legacy_retry_erased_the_cursor() {
    let mut book = ReviewThreads::new("change".into());
    let post = start("Question");
    let thread = post.thread_id().clone();
    book.post(post).unwrap();
    answer(&mut book, &thread, "Done");
    book.answered.clear();
    book.migrate_answered_positions();
    assert!(book.new_messages().is_empty());
    book.post(Post::reply(thread, "Later question".into()))
        .unwrap();
    book.answered.clear();
    book.migrate_answered_positions();
    assert_eq!(book.new_messages()[0].messages.len(), 3);
}

#[test]
fn legacy_answers_do_not_become_ready_again_after_a_final_read_or_upgrade() {
    let mut book = ReviewThreads::new("change".into());
    let post = start("Question");
    let thread = post.thread_id().clone();
    book.post(post).unwrap();
    book.post(agent_reply(&thread, "Legacy answer")).unwrap();
    // Version 2 advanced this cursor again on the agent's final empty fetch.
    book.readers.insert(
        "old-agent".into(),
        BTreeMap::from([(thread.clone(), book.sequence())]),
    );
    book.recover_retrieved_comments();
    book.migrate_answered_positions();
    for book in [
        book.clone(),
        serde_json::from_value(serde_json::to_value(book).unwrap()).unwrap(),
    ] {
        assert!(!book.thread(&thread).unwrap().is_waiting());
        assert!(!book.has_new_messages());
        assert!(book.new_messages().is_empty());
        assert_eq!(book.pending_comment_sequence(), None);
        assert!(book.retry(&thread).is_err());
        let mut followed_up = book;
        followed_up
            .post(Post::reply(thread.clone(), "Later question".into()))
            .unwrap();
        assert!(followed_up.thread(&thread).unwrap().is_waiting());
        assert_eq!(followed_up.new_messages().len(), 1);
    }
}

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

fn answer(book: &mut ReviewThreads, thread: &ThreadId, text: &str) -> MessageId {
    let comment = book
        .thread(thread)
        .unwrap()
        .last_comment()
        .unwrap()
        .id
        .clone();
    let post = Post::answer(
        thread.clone(),
        MessageId::parse(&uuid::Uuid::new_v4().to_string()).unwrap(),
        text.into(),
        comment,
    );
    book.answer(post).unwrap()
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
    answer(&mut book, &thread, "First answer");
    answer(&mut book, &other_thread, "Other answer");
    assert!(!book.has_new_messages());

    book.post(Post::reply(thread.clone(), "And this part?".into()))
        .unwrap();
    let updated = book.new_messages();
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
    assert_eq!(book.new_messages().len(), 1);
}

#[test]
fn a_reply_acknowledges_only_its_fetched_snapshot_and_retries_leave_new_comments_pending() {
    let mut book = ReviewThreads::new("change".into());
    let first = start("First");
    let thread = first.thread_id().clone();
    let fetched = book.post(first).unwrap();
    let reply = Post::answer(
        thread.clone(),
        MessageId::parse(&uuid::Uuid::new_v4().to_string()).unwrap(),
        "Answer to first".into(),
        fetched,
    );
    book.post(Post::reply(thread.clone(), "Arrived during read".into()))
        .unwrap();
    book.answer(reply.clone()).unwrap();
    book.answer(reply).unwrap();
    assert!(book.has_new_messages());
    assert!(book.thread(&thread).unwrap().is_waiting());
    let original = book.clone();
    assert_eq!(book.new_messages(), book.new_messages());
    assert_eq!(book, original, "retrieval does not acknowledge delivery");
    answer(&mut book, &thread, "Answer to follow-up");
    assert!(!book.has_new_messages());
    assert!(!book.thread(&thread).unwrap().is_waiting());
    assert!(book.retry(&thread).is_err());
    assert!(book.new_messages().is_empty());
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
fn invalid_answers_do_not_post_or_consume_pending_comments() {
    let mut book = ReviewThreads::new("change".into());
    let first = start("First thread");
    let thread = first.thread_id().clone();
    let first_id = book.post(first).unwrap();
    let other = book.post(start("Different thread")).unwrap();
    let agent = book.post(agent_reply(&thread, "Legacy answer")).unwrap();
    for invalid in [other, agent] {
        let before = book.clone();
        let reply = Post::answer(
            thread.clone(),
            MessageId::parse(&uuid::Uuid::new_v4().to_string()).unwrap(),
            "Invalid answer".into(),
            invalid,
        );
        assert!(book.answer(reply).is_err());
        assert_eq!(book, before);
    }
    let before = book.clone();
    let blank = Post::answer(
        thread,
        MessageId::parse(&uuid::Uuid::new_v4().to_string()).unwrap(),
        " \n ".into(),
        first_id,
    );
    assert!(book.answer(blank).is_err());
    assert_eq!(book, before);
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
fn resolution_and_reviewer_reads_are_independent_of_answer_acknowledgement() {
    let mut book = ReviewThreads::new("change".into());
    let post = start("Question");
    let thread = post.thread_id().clone();
    book.post(post).unwrap();
    answer(&mut book, &thread, "First answer");
    let displayed = book.sequence();
    book.set_resolution(&thread, Resolution::Resolved).unwrap();
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
    assert!(!book.has_new_messages());
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
    book.post(Post::reply(thread.clone(), "Unanswered follow-up".into()))
        .unwrap();
    book.mark_replies_read(&[second.clone(), question_id]);
    let mut book: ReviewThreads =
        serde_json::from_str(&serde_json::to_string(&book).unwrap()).unwrap();
    assert!(book.thread(&thread).unwrap().has_unread_reply(&first));
    assert!(!book.thread(&thread).unwrap().has_unread_reply(&second));
    assert!(book.has_new_messages());
    let late = book.post(agent_reply(&thread, "Late answer")).unwrap();
    book.mark_replies_read(&[first, second]);
    assert!(book.thread(&thread).unwrap().has_unread_reply(&late));
    assert_eq!(book.counts().unread, 1);
    book.mark_replies_read(&[late]);
    assert_eq!(book.counts().unread, 0);
    assert_eq!(book.counts().open, 1);
}
