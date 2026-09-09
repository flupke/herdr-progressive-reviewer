use super::*;

#[test]
fn clicking_reply_scrolls_only_enough_to_reveal_the_editor_bottom() {
    for (height, scroll_up) in [(40, 0), (16, 4)] {
        let mut fixture = CommentFixture::new();
        fixture.add("Question");
        fixture.answer(&"An answer that takes several rows.\n\n".repeat(8));
        fixture
            .registry
            .publish(DiffViewportChanged { width: 78, height })
            .unwrap();
        fixture
            .registry
            .dispatch_hovered_input(
                &EventEnvelope::new(PointerInput {
                    kind: PointerInputKind::Scroll(isize::MAX),
                    position: None,
                }),
                fixture.target,
            )
            .unwrap();
        fixture
            .registry
            .dispatch_hovered_input(
                &EventEnvelope::new(PointerInput {
                    kind: PointerInputKind::Scroll(-scroll_up),
                    position: None,
                }),
                fixture.target,
            )
            .unwrap();
        let area = Rect::new(0, 0, 80, height + 2);
        let buffer = fixture.render_in(area);
        let (row, column) = CommentFixture::text_position(&buffer, "Reply…");
        let before = fixture
            .component()
            .displayed_document()
            .unwrap()
            .document
            .scroll;
        fixture.click(row, column);
        let after = fixture
            .component()
            .displayed_document()
            .unwrap()
            .document
            .scroll;
        let buffer = fixture.render_in(area);
        let (post_row, post_column) = CommentFixture::text_position(&buffer, "Post");
        if scroll_up == 0 {
            assert_eq!(after, before, "an editor that fits must not move the diff");
        } else {
            assert!(after > before);
            assert_eq!(
                post_row, height,
                "only reveal the bottom row, without extra scrolling"
            );
        }
        fixture.click(post_row - 3, post_column);
        assert_eq!(
            fixture
                .component()
                .displayed_document()
                .unwrap()
                .document
                .scroll,
            after,
            "clicking inside the editor must not move it"
        );
    }
}
