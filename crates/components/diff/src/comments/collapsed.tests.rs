use super::*;

#[test]
fn resolved_summary_has_its_own_box_without_enclosing_the_file_diff() {
    for overlapping_open_thread in [false, true] {
        let mut fixture = CommentFixture::new();
        fixture.add("Resolve this question");
        if overlapping_open_thread {
            fixture.add("Keep this conversation open");
        }
        let mut book = fixture.book().clone();
        let id = book.threads()[0].id.clone();
        book.set_resolution(&id, review_threads::Resolution::Resolved)
            .unwrap();
        fixture
            .registry
            .publish(ui_events::ReviewThreadsLoaded {
                review_unit: book.review_unit.clone(),
                result: Ok(book.clone()),
            })
            .unwrap();
        fixture.key(Key::First);
        let area = Rect::new(0, 0, 80, 80);
        let buffer = fixture.render_in(area);
        let border_column = u16::try_from(
            fixture
                .component()
                .selected_document()
                .unwrap()
                .document
                .diff
                .line_number_width()
                + 3,
        )
        .unwrap();
        let (code_row, _) = CommentFixture::text_position(&buffer, "changed");
        let (summary_row, _) =
            CommentFixture::text_position(&buffer, "Resolved · Resolve this question");
        assert!(code_row < summary_row);
        assert_eq!(
            buffer[(border_column, summary_row - 1)].symbol(),
            "╭",
            "the resolved box starts immediately above its summary"
        );
        assert_eq!(buffer[(border_column, summary_row + 2)].symbol(), "╰");
        assert_eq!(
            buffer[(border_column, code_row)].symbol() == "│",
            overlapping_open_thread,
            "only the unresolved conversation may frame code"
        );
        let (row, column) = CommentFixture::text_position(&buffer, "Unresolve thread");
        let actions = fixture.click(row, column);
        assert!(matches!(
            actions.as_slice(),
            [Action::Thread(
                review_threads::ThreadCommand::SetResolution {
                    resolution: review_threads::Resolution::Open,
                    ..
                }
            )]
        ));
        book.set_resolution(&id, review_threads::Resolution::Open)
            .unwrap();
        fixture
            .registry
            .publish(ui_events::ReviewThreadsLoaded {
                review_unit: book.review_unit.clone(),
                result: Ok(book),
            })
            .unwrap();
        fixture.key(Key::First);
        let buffer = fixture.render_in(area);
        let (code_row, _) = CommentFixture::text_position(&buffer, "changed");
        assert_eq!(buffer[(border_column, code_row)].symbol(), "│");
        CommentFixture::text_position(&buffer, "Reply…");
    }
}
