use super::*;
use crate::{Author, MessageId, Post, Resolution, ReviewThreads, ThreadSubject};

fn question(id: &str, version: u32) -> AskedUnder {
    AskedUnder::Question {
        question: id.into(),
        version,
    }
}

fn reply_to_last_comment(book: &mut ReviewThreads, round: &str, text: &str) -> MessageId {
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
fn the_first_message_starts_the_rounds_conversation_and_later_ones_join_it() {
    let mut book = ReviewThreads::new("change".into());
    book.post(Post::to_round(
        "round-1",
        "Why a lock here?".into(),
        Some(question("q-lock", 2)),
        Some("the store takes a lock".into()),
    ))
    .unwrap();
    book.post(Post::to_round(
        "round-1",
        "And the design?".into(),
        Some(AskedUnder::Design),
        None,
    ))
    .unwrap();

    assert_eq!(book.threads().len(), 1);
    let thread = book.round_conversation("round-1").unwrap();
    assert_eq!(
        thread.subject,
        ThreadSubject::Round {
            round: "round-1".into()
        }
    );
    assert_eq!(thread.code(), None);
    assert_eq!(thread.path(), None);
    let [first, second] = thread.messages.as_slice() else {
        panic!("the conversation holds both messages");
    };
    assert_eq!(first.author, Author::Reviewer);
    assert_eq!(first.asked_under, Some(question("q-lock", 2)));
    assert_eq!(first.quote.as_deref(), Some("the store takes a lock"));
    assert_eq!(second.asked_under, Some(AskedUnder::Design));
    assert_eq!(second.quote, None);
    assert!(first.posted_at_ms.is_some());
    assert!(first.sequence() < second.sequence());
}

#[test]
fn each_round_has_its_own_conversation() {
    let mut book = ReviewThreads::new("change".into());
    book.post(Post::to_round("round-1", "First round".into(), None, None))
        .unwrap();
    book.post(Post::to_round(
        "round-2",
        "After a Reset".into(),
        Some(question("q-1", 1)),
        None,
    ))
    .unwrap();

    assert_eq!(book.threads().len(), 2);
    let first = book.round_conversation("round-1").unwrap();
    let second = book.round_conversation("round-2").unwrap();
    assert_ne!(first.id, second.id);
    assert_eq!(second.messages[0].text, "After a Reset");
    assert!(book.round_conversation("round-3").is_none());
}

#[test]
fn a_retried_message_is_posted_once() {
    let mut book = ReviewThreads::new("change".into());
    let post = Post::to_round("round-1", "Once".into(), Some(question("q-1", 1)), None);
    let id = book.post(post.clone()).unwrap();
    assert_eq!(book.post(post).unwrap(), id);
    assert_eq!(
        book.round_conversation("round-1").unwrap().messages.len(),
        1
    );
}

#[test]
fn a_message_waits_for_the_agent_until_a_reply_takes_it_up() {
    let mut book = ReviewThreads::new("change".into());
    book.post(Post::to_round(
        "round-1",
        "Is this safe?".into(),
        Some(question("q-1", 1)),
        None,
    ))
    .unwrap();
    let pending = book.new_messages();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].round(), Some("round-1"));

    reply_to_last_comment(&mut book, "round-1", "Yes: the lock covers it.");
    assert!(!book.has_new_messages());
    let thread = book.round_conversation("round-1").unwrap();
    assert!(thread.has_unread_replies());
    assert!(thread.is_answered(&thread.messages[0]));
}

#[test]
fn a_message_reopens_a_resolved_round_conversation() {
    let mut book = ReviewThreads::new("change".into());
    book.post(Post::to_round("round-1", "First".into(), None, None))
        .unwrap();
    reply_to_last_comment(&mut book, "round-1", "Answered");
    let thread = book.round_conversation("round-1").unwrap().id.clone();
    book.set_resolution(&thread, Resolution::Resolved).unwrap();

    book.post(Post::to_round("round-1", "One more".into(), None, None))
        .unwrap();
    assert_eq!(book.thread(&thread).unwrap().resolution, Resolution::Open);
    assert!(book.has_new_messages());
}

#[test]
fn a_round_conversation_survives_serialization_beside_code_threads() {
    let mut book = ReviewThreads::new("change".into());
    book.post(Post::to_round(
        "round-1",
        "Quoted".into(),
        Some(AskedUnder::Conclusion {
            conclusion: "request-9".into(),
        }),
        Some("a passage".into()),
    ))
    .unwrap();
    let json = serde_json::to_value(&book).unwrap();
    assert_eq!(json["threads"][0]["round"], "round-1");
    assert_eq!(
        json["threads"][0]["messages"][0]["asked_under"],
        serde_json::json!({"stage": "conclusion", "conclusion": "request-9"})
    );
    let restored: ReviewThreads = serde_json::from_value(json).unwrap();
    assert_eq!(restored, book);
}

#[test]
fn a_round_message_sent_again_with_its_identity_is_posted_once() {
    let mut book = ReviewThreads::new("change".into());
    let id = MessageId::parse(&uuid::Uuid::new_v4().to_string()).unwrap();
    let post = || {
        Post::to_round(
            "round-1",
            "Why a lock here?".into(),
            Some(question("q-lock", 1)),
            None,
        )
        .with_id(id.clone())
    };

    assert_eq!(book.post(post()), Ok(id.clone()));
    assert_eq!(book.post(post()), Ok(id.clone()));

    let thread = book.round_conversation("round-1").unwrap();
    assert_eq!(thread.messages.len(), 1);
    assert_eq!(thread.messages[0].id, id);
}

#[test]
fn postings_take_the_times_given_for_their_sequence() {
    let mut book = ReviewThreads::new("change".into());
    for text in ["Why a lock here?", "And the design?"] {
        book.post(Post::to_round("round-1", text.into(), None, None))
            .unwrap();
    }
    book.stamp_postings(|sequence| 1_000 + sequence);

    let messages = &book.round_conversation("round-1").unwrap().messages;
    let times: Vec<_> = messages
        .iter()
        .map(|message| message.posted_at_ms)
        .collect();
    let sequences: Vec<_> = messages
        .iter()
        .map(|message| Some(1_000 + message.sequence()))
        .collect();
    assert_eq!(times, sequences);
    assert_ne!(times[0], times[1]);
}
