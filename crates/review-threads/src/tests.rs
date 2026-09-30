use super::*;
use review_source::DiffRangeAnchor;

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
fn a_file_keeps_each_new_thread_draft_and_saving_replaces_only_the_same_one() {
    let book = ReviewThreads::new("change".into());
    let mut drafts = crate::SavedDrafts::default();
    let source = Arc::new(ThreadSource {
        anchor: start("unused").source.as_ref().unwrap().anchor.clone(),
        excerpt: "+original".into(),
    });
    let mut first = crate::Draft::start("gone.rs".into(), source.clone());
    let mut second = crate::Draft::start("gone.rs".into(), source);
    first.text = "First".into();
    second.text = "Second".into();
    drafts.save(first.clone(), &book).unwrap();
    drafts.save(second.clone(), &book).unwrap();
    first.text = "First, edited".into();
    drafts.save(first.clone(), &book).unwrap();
    let mut texts = drafts
        .drafts()
        .iter()
        .map(|draft| draft.text.as_str())
        .collect::<Vec<_>>();
    texts.sort_unstable();
    assert_eq!(texts, ["First, edited", "Second"]);
    drafts.discard(second.thread_id());
    assert_eq!(drafts.drafts(), [first]);
}

#[test]
fn a_posted_draft_is_no_longer_saved_and_cannot_be_saved_again() {
    let mut book = ReviewThreads::new("change".into());
    let mut drafts = crate::SavedDrafts::default();
    let source = Arc::new(ThreadSource {
        anchor: start("unused").source.as_ref().unwrap().anchor.clone(),
        excerpt: "+original".into(),
    });
    let mut posted = crate::Draft::start("file.rs".into(), source.clone());
    let mut kept = crate::Draft::start("file.rs".into(), source);
    posted.text = "Posted".into();
    kept.text = "Kept".into();
    drafts.save(posted.clone(), &book).unwrap();
    drafts.save(kept.clone(), &book).unwrap();
    book.post(posted.post()).unwrap();
    drafts.forget_posted(&book);
    assert_eq!(drafts.drafts(), [kept]);
    assert!(drafts.save(posted, &book).is_err());
}

fn start(text: &str) -> Post {
    Post::start(
        DiffRangeAnchor {
            source_checkpoint: "initial".into(),
            old_path: None,
            new_path: Some("gone.rs".into()),
            old_lines: None,
            new_lines: Some(0..1),
            target_kind: review_source::AnchorKind::Lines,
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
