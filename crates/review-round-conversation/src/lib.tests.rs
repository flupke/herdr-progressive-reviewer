use review_threads::Post;
use serde_json::json;

use super::*;

fn talk(book: &mut ReviewThreads, round: &str, text: &str, asked_under: Option<AskedUnder>) {
    book.post(Post::to_round(round, text.into(), asked_under, None))
        .unwrap();
}

fn reply(book: &mut ReviewThreads, round: &str, text: &str) -> MessageId {
    let thread = book.round_conversation(round).unwrap();
    let post = Post::answer(
        thread.id.clone(),
        MessageId::parse(&uuid::Uuid::new_v4().to_string()).unwrap(),
        text.into(),
        thread.last_comment().unwrap().id.clone(),
    );
    book.answer(post).unwrap()
}

#[test]
fn a_round_without_messages_has_an_empty_conversation() {
    let book = ReviewThreads::new("change".into());
    let conversation = RoundConversation::new(&book, "round-1", None);
    assert_eq!(conversation.thread, None);
    assert_eq!(conversation.round, "round-1");
    assert!(conversation.messages.is_empty());
    assert_eq!(conversation.unread, 0);
    assert!(!conversation.can_retry);
}

#[test]
fn the_conversation_lists_its_messages_in_order_with_their_question_quote_and_delivery() {
    let mut book = ReviewThreads::new("change".into());
    book.post(Post::to_round(
        "round-1",
        "Why a lock?".into(),
        Some(AskedUnder::Question {
            question: "q-lock".into(),
            version: 2,
            number: None,
        }),
        Some("the store takes a lock".into()),
    ))
    .unwrap();
    let answer = reply(&mut book, "round-1", "It guards the index.");
    talk(
        &mut book,
        "round-1",
        "And the design?",
        Some(AskedUnder::Design),
    );
    talk(&mut book, "round-2", "Another round", None);

    let conversation = RoundConversation::new(&book, "round-1", None);
    let thread = book.round_conversation("round-1").unwrap();
    assert_eq!(conversation.thread.as_ref(), Some(&thread.id));
    let [asked, answered, waiting] = conversation.messages.as_slice() else {
        panic!("three messages in round-1, got {:?}", conversation.messages);
    };
    assert_eq!(
        (asked.author, asked.text.as_str(), asked.quote.as_deref()),
        (
            Author::Reviewer,
            "Why a lock?",
            Some("the store takes a lock")
        )
    );
    assert_eq!(
        asked.asked_under,
        Some(AskedUnder::Question {
            question: "q-lock".into(),
            version: 2,
            number: None,
        })
    );
    assert_eq!(asked.delivery, Some(Delivery::Answered));
    assert_eq!(answered.id, answer);
    assert_eq!(answered.author, Author::Agent);
    assert_eq!(answered.delivery, None);
    assert!(answered.unread);
    assert_eq!(waiting.delivery, Some(Delivery::Waiting));
    assert!(asked.posted_at_ms.is_some() && answered.posted_at_ms >= asked.posted_at_ms);
    assert_eq!(conversation.unread, 1);
    assert!(conversation.can_retry);
    assert_eq!(conversation.read_through, book.sequence());
}

#[test]
fn reading_the_reply_clears_the_unread_count() {
    let mut book = ReviewThreads::new("change".into());
    talk(&mut book, "round-1", "Why?", None);
    reply(&mut book, "round-1", "Because.");
    let conversation = RoundConversation::new(&book, "round-1", None);
    book.mark_read(
        conversation.thread.as_ref().unwrap(),
        conversation.read_through,
    )
    .unwrap();
    let conversation = RoundConversation::new(&book, "round-1", None);
    assert_eq!(conversation.unread, 0);
    assert!(!conversation.messages[1].unread);
    assert!(!conversation.can_retry);
}

#[test]
fn a_failed_wakeup_marks_the_messages_it_covered_as_not_delivered() {
    let mut book = ReviewThreads::new("change".into());
    talk(&mut book, "round-1", "Covered", None);
    let through = book.sequence();
    talk(&mut book, "round-1", "Posted after the wakeup", None);
    let failure = WakeupFailure {
        through,
        error: "The agent did not start on the prompt.".into(),
    };

    let conversation = RoundConversation::new(&book, "round-1", Some(&failure));
    assert_eq!(
        conversation.messages[0].delivery,
        Some(Delivery::NotDelivered {
            error: "The agent did not start on the prompt.".into()
        })
    );
    assert_eq!(conversation.messages[1].delivery, Some(Delivery::Waiting));

    reply(&mut book, "round-1", "Got both");
    let conversation = RoundConversation::new(&book, "round-1", Some(&failure));
    assert_eq!(conversation.messages[0].delivery, Some(Delivery::Answered));
}

#[test]
fn the_conversation_serializes_as_plain_data() {
    let mut book = ReviewThreads::new("change".into());
    talk(
        &mut book,
        "round-1",
        "Done?",
        Some(AskedUnder::Conclusion {
            conclusion: "request-9".into(),
        }),
    );
    let value = serde_json::to_value(RoundConversation::new(&book, "round-1", None)).unwrap();
    let message = &value["messages"][0];
    assert_eq!(message["author"], "reviewer");
    assert_eq!(message["delivery"], json!({"state": "waiting"}));
    assert_eq!(
        message["asked_under"],
        json!({"stage": "conclusion", "conclusion": "request-9"})
    );
    assert_eq!(value["unread"], 0);
}
