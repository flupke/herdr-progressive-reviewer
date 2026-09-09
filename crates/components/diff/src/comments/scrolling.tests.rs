use super::*;
use std::fmt::Write as _;

#[test]
fn clicking_reply_scrolls_only_enough_to_reveal_the_editor_bottom() {
    for (height, scroll_up) in [(40, 0), (16, 4)] {
        let mut fixture = CommentFixture::new();
        fixture.add("Question");
        let mut answer = String::new();
        for i in 0..8 {
            writeln!(&mut answer, "Answer row {i}.\n").unwrap();
        }
        fixture.answer(&answer);
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
        let (before, _) = CommentFixture::text_position(&buffer, "Answer row 7.");
        fixture.click(row, column);
        let buffer = fixture.render_in(area);
        let (after, _) = CommentFixture::text_position(&buffer, "Answer row 7.");
        let (post_row, post_column) = CommentFixture::text_position(&buffer, "Post");
        if scroll_up == 0 {
            assert_eq!(after, before, "an editor that fits must not move the diff");
        } else {
            assert!(after < before, "scrolling down moves the answer upward");
            assert_eq!(
                post_row, height,
                "only reveal the bottom row, without extra scrolling"
            );
        }
        fixture.click(post_row - 3, post_column);
        let after_click = fixture.render_in(area);
        assert_eq!(
            CommentFixture::text_position(&after_click, "Post").0,
            post_row,
            "clicking inside the editor must not move it"
        );
        assert_eq!(
            CommentFixture::text_position(&after_click, "Answer row 7.").0,
            after
        );
    }
}
