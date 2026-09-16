use super::*;
use review_guide::{DiffRangeAnchor, GuideAnchorKind};

#[test]
fn saved_diff_renders_line_numbers_changes_and_syntax_with_or_without_full_source() {
    for full_source in [true, false] {
        let mut fixture = CommentFixture::new();
        let post = Post::start(
            DiffRangeAnchor {
                source_checkpoint: "original".into(),
                old_path: Some("src/lib.rs".into()),
                new_path: Some("src/lib.rs".into()),
                old_lines: Some(0..1),
                new_lines: Some(0..1),
                target_kind: GuideAnchorKind::Lines,
                source_hunk_count: 1,
                old_content: full_source.then(|| b"fn old() {}\n".to_vec()),
                new_content: full_source.then(|| b"fn new() {}\n".to_vec()),
                diff_hash: String::new(),
            },
            "@@ -1 +1 @@\n-fn old() {}\n+fn new() {}".into(),
            "Why change it?".into(),
        );
        let thread_id = post.thread_id().clone();
        let mut book = fixture.book().clone();
        book.post(post).unwrap();
        fixture.publish_book(book);
        fixture
            .registry
            .publish(ui_events::ReviewNavigationChanged(
                ui_events::ReviewNavigation::Threads,
            ))
            .unwrap();
        fixture
            .registry
            .publish(ui_events::ThreadSelectionChanged {
                thread_id: Some(thread_id),
            })
            .unwrap();
        let buffer = fixture.render_thread();
        let palette = Theme::default().palette;
        let (old_row, _) = CommentFixture::text_position(&buffer, "fn old() {}");
        let (new_row, _) = CommentFixture::text_position(&buffer, "fn new() {}");
        assert_eq!(new_row, old_row + 1);
        for (row, text, marker, background) in [
            (
                old_row,
                "fn old() {}",
                palette.deletion,
                palette.deletion_bg,
            ),
            (
                new_row,
                "fn new() {}",
                palette.insertion,
                palette.insertion_bg,
            ),
        ] {
            let (_, column) = CommentFixture::text_position(&buffer, text);
            let gutter = (0..column)
                .map(|x| buffer[(x, row)].symbol())
                .collect::<String>();
            assert_eq!(
                gutter
                    .chars()
                    .filter(char::is_ascii_digit)
                    .collect::<String>(),
                "1",
                "{gutter}"
            );
            assert!(
                (0..column)
                    .any(|x| buffer[(x, row)].symbol() == "▌" && buffer[(x, row)].fg == marker)
            );
            assert_eq!(buffer[(column, row)].bg, background);
            let colors = (column..column + u16::try_from(text.len()).unwrap())
                .map(|x| buffer[(x, row)].fg)
                .collect::<std::collections::HashSet<_>>();
            assert!(
                colors.len() > 1,
                "saved code must retain syntax colors: {buffer:?}"
            );
        }
    }
}
