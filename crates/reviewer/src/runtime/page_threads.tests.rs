use review_explore_page::CommandReply;
use review_threads::{AskedUnder, Post, ReviewThreads, ThreadCommand, WakeupFailure};
use ui_events::{ReviewThreadsLoaded, ThreadPostFinished};

use super::*;

fn post(text: &str) -> ThreadCommand {
    ThreadCommand::Post {
        review_unit: "review-1".into(),
        post: Post::to_round("round-1", text.into(), Some(AskedUnder::Design), None),
    }
}

fn posted(command: &ThreadCommand, result: Result<(), String>) -> comments::Event {
    let ThreadCommand::Post { review_unit, post } = command else {
        panic!("not a post");
    };
    comments::Event::Posted(ThreadPostFinished {
        review_unit: review_unit.clone(),
        message_id: post.message().id.clone(),
        result,
    })
}

#[test]
fn a_page_post_is_answered_once_the_worker_reports_it() {
    let threads = PageThreads::default();
    let (stored, mut stored_reply) = CommandReply::channel();
    let (refused, mut refused_reply) = CommandReply::channel();
    let mut delivered = Vec::new();
    let first = post("Why a lock?");
    let second = post("And the design?");
    threads.take(first.clone(), stored, |command| delivered.push(command));
    threads.take(second.clone(), refused, |command| delivered.push(command));

    assert_eq!(delivered.len(), 2);
    assert!(
        stored_reply.try_recv().is_err(),
        "a post waits for the worker"
    );

    threads.observe(&posted(&first, Ok(())));
    threads.observe(&posted(&second, Err("disk full".into())));
    assert_eq!(stored_reply.try_recv(), Ok(Ok(())));
    assert_eq!(
        refused_reply.try_recv(),
        Ok(Err(CommandRefusal::Failed("disk full".into())))
    );
}

#[test]
fn another_command_is_answered_once_handed_to_the_worker() {
    let threads = PageThreads::default();
    let (reply, mut replied) = CommandReply::channel();
    let mut delivered = Vec::new();
    let retry = ThreadCommand::Retry {
        review_unit: "review-1".into(),
        thread_id: serde_json::from_str("\"explore-round-round-1\"").unwrap(),
    };

    threads.take(retry, reply, |command| delivered.push(command));

    assert_eq!(delivered.len(), 1);
    assert_eq!(replied.try_recv(), Ok(Ok(())));
}

#[test]
fn the_page_sees_the_threads_and_the_wakeups_the_worker_publishes() {
    let threads = PageThreads::default();
    let feed = threads.subscribe();
    let mut book = ReviewThreads::new("review-1".into());
    let ThreadCommand::Post { post, .. } = post("Why a lock?") else {
        unreachable!()
    };
    book.post(post).unwrap();

    threads.observe(&comments::Event::Loaded(ReviewThreadsLoaded {
        review_unit: "review-1".into(),
        result: Ok(book),
        drafts: review_threads::SavedDrafts::default(),
    }));
    threads.observe(&comments::Event::Wakeup {
        review_unit: "review-1".into(),
        failure: Some(WakeupFailure {
            through: 1,
            error: "No agent is focused".into(),
        }),
    });

    let conversation = feed.conversation(&"review-1".into(), "round-1");
    assert_eq!(conversation.messages.len(), 1);
    assert!(matches!(
        conversation.messages[0].delivery,
        Some(review_round_conversation::Delivery::NotDelivered { .. })
    ));
}
