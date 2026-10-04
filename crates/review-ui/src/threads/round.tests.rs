use super::*;
use review_threads::AskedUnder;

impl ThreadUi {
    /// Add the conversation of an Explore round, with one message and the agent's reply.
    fn talk_in_round(&mut self) -> ThreadId {
        self.book
            .post(Post::to_round(
                "round-1",
                "Why does the store lock here?".into(),
                Some(AskedUnder::Question {
                    question: "q-lock".into(),
                    version: 2,
                }),
                Some("the store takes a lock".into()),
            ))
            .unwrap();
        let thread = self.book.round_conversation("round-1").unwrap();
        let (id, comment) = (thread.id.clone(), thread.messages[0].id.clone());
        self.book
            .answer(Post::answer(
                id.clone(),
                MessageId::parse("5d0f7a52-8a2c-4f0e-9c1b-3a4d5e6f7a8b").unwrap(),
                "It keeps two reviewers from writing at once.".into(),
                comment,
            ))
            .unwrap();
        self.publish_book();
        id
    }
}

#[test]
fn the_threads_list_shows_a_round_conversation_and_opens_it_like_any_thread() {
    let mut ui = ThreadUi::new(110);
    let thread = ui.talk_in_round();
    ui.key(Key::Char('t'));
    let list = ui.text();
    assert!(list.contains("Why does the store lock"), "{list}");
    assert!(list.contains("Round conversation"), "{list}");

    ui.key(Key::Down);
    ui.key(Key::Down);
    ui.key(Key::Enter);
    let conversation = ui.text();
    for shown in [
        "Round conversation of an Explore round",
        "under question q-lock, version 2",
        "> the store takes a lock",
        "It keeps two reviewers from writing at once.",
        "Write in this conversation from the Explore page.",
        "New reply",
    ] {
        assert!(conversation.contains(shown), "{shown}: {conversation}");
    }
    assert!(!conversation.contains("Reply…"), "{conversation}");

    for key in [Key::Char('a'), Key::Char('A')] {
        ui.key(key);
        assert!(!ui.text().contains("Ctrl-Enter post"), "{}", ui.text());
    }
    let read = ui.key(Key::Char('u'));
    assert!(
        matches!(
            read.as_slice(),
            [Action::Thread(ThreadCommand::MarkRead { thread_id, .. })] if thread_id == &thread
        ),
        "{read:?}"
    );
    assert!(!ui.book.thread(&thread).unwrap().has_unread_replies());
}
