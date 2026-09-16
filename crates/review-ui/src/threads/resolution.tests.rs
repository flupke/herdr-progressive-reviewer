use super::*;

impl ThreadUi {
    fn add_thread(&mut self, question: &str) -> usize {
        let original = self.book.thread(&self.ids[0]).unwrap();
        let post = Post::start(
            original.anchor.clone(),
            original.excerpt.clone(),
            question.into(),
        );
        self.ids.push(post.thread_id().clone());
        self.book.post(post).unwrap();
        self.publish_book();
        self.ids.len() - 1
    }
}

#[test]
fn resolving_hides_the_current_thread_and_clears_the_last_conversation() {
    for width in [48, 110] {
        let mut ui = ThreadUi::new(width);
        ui.key(Key::Char('t'));
        ui.key(Key::Enter);
        ui.key(Key::Last);
        ui.click_text("Resolve thread");
        let text = ui.text();
        assert!(!text.contains("Explain this branch"), "{text}");
        assert!(text.contains("Keep the removed"), "{text}");
        assert_eq!(
            ui.book.thread(&ui.ids[0]).unwrap().resolution,
            Resolution::Resolved
        );

        ui.key(Key::Char('r'));
        ui.assert_no_conversation();
        assert!(!ui.text().contains("Reply…"));
        assert_eq!(ui.book.counts().open, 0);
        ui.publish_book();
        ui.assert_no_conversation();
    }
}

#[test]
fn resolving_prefers_the_first_unresolved_thread_with_unread_answers() {
    for width in [48, 110] {
        let mut ui = ThreadUi::new(width);
        let first_unread = ui.add_thread("First unread question");
        let second_unread = ui.add_thread("Second unread question");
        ui.answer(
            first_unread,
            "00000000-0000-4000-8000-000000000101",
            "First unread answer",
        );
        ui.answer(
            second_unread,
            "00000000-0000-4000-8000-000000000102",
            "Second unread answer",
        );
        ui.key(Key::Char('t'));
        ui.key(Key::Enter);
        ui.key(Key::Char('r'));
        assert!(ui.text().contains("First unread answer"), "{}", ui.text());
        assert!(!ui.text().contains("Second unread answer"));
        ui.key(Key::Char('r'));
        assert!(ui.text().contains("Second unread answer"), "{}", ui.text());
        ui.key(Key::Char('r'));
        assert!(ui.text().contains("Keep the removed"), "{}", ui.text());
        assert!(!ui.text().contains("Second unread answer"));
        assert_eq!(ui.book.counts().open, 1);
    }
}

#[test]
fn resolving_advances_in_list_order_then_wraps_when_no_answers_are_unread() {
    for width in [48, 110] {
        let mut ui = ThreadUi::new(width);
        ui.add_thread("Last remaining question");
        ui.key(Key::Char('t'));
        ui.key(Key::Down);
        ui.key(Key::Enter);
        ui.key(Key::Char('r'));
        ui.key(Key::Char('r'));
        assert_eq!(
            ui.book.thread(&ui.ids[1]).unwrap().resolution,
            Resolution::Resolved
        );
        assert_eq!(
            ui.book.thread(&ui.ids[2]).unwrap().resolution,
            Resolution::Resolved
        );
        assert_eq!(
            ui.book.thread(&ui.ids[0]).unwrap().resolution,
            Resolution::Open
        );
        assert!(ui.text().contains("Explain this branch"), "{}", ui.text());
        ui.key(Key::Char('r'));
        ui.assert_no_conversation();
    }
}

#[test]
fn all_keeps_history_but_does_not_reopen_the_last_resolved_conversation() {
    for width in [48, 110] {
        let mut ui = ThreadUi::new(width);
        ui.key(Key::Char('t'));
        ui.key(Key::Char('2'));
        ui.key(Key::Enter);
        ui.key(Key::Char('r'));
        ui.key(Key::Char('r'));
        for _ in 0..2 {
            ui.publish_book();
            let text = ui.text();
            assert!(!text.contains("first original line"), "{text}");
            assert!(!text.contains("Reply…"), "{text}");
            assert!(ui.key(Key::Char('r')).is_empty());
        }
        ui.answer(
            0,
            "00000000-0000-4000-8000-000000000103",
            "Late resolved answer",
        );
        assert!(!ui.text().contains("Late resolved answer"));
        ui.key(Key::Tab);
        ui.key(Key::First);
        ui.key(Key::Enter);
        assert!(ui.text().contains("Late resolved answer"), "{}", ui.text());
        ui.key(Key::Char('r'));
        assert_eq!(
            ui.book.thread(&ui.ids[0]).unwrap().resolution,
            Resolution::Open
        );
    }
}

#[test]
fn resolving_the_last_search_match_clears_detail_without_leaving_the_search() {
    for width in [48, 110] {
        let mut ui = ThreadUi::new(width);
        ui.key(Key::Char('t'));
        ui.key(Key::Char('/'));
        ui.paste("gone.rs");
        ui.key(Key::Enter);
        ui.key(Key::Enter);
        ui.key(Key::Char('r'));
        ui.assert_no_conversation();
        assert_eq!(ui.book.counts().open, 1);
        ui.key(Key::Tab);
        assert!(ui.text().contains("/gone.rs"), "{}", ui.text());
        assert!(!ui.text().contains("Keep the removed"));
    }
}
