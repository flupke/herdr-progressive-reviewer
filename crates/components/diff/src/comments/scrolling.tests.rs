use super::*;
use std::fmt::Write as _;

fn post_preserving_visible_line(fixture: &mut CommentFixture, lines: &[String]) {
    let area = Rect::new(0, 0, 80, 12);
    let before = fixture.render_in(area);
    let (text, row) = lines
        .iter()
        .find_map(|text| {
            let row = before.content().chunks(80).position(|cells| {
                cells
                    .iter()
                    .map(ratatui::buffer::Cell::symbol)
                    .collect::<String>()
                    .contains(text)
            })?;
            Some((text, row))
        })
        .expect("a stable line must be visible before posting");
    fixture.key(Key::ControlEnter);
    let after = fixture.render_in(area);
    assert_eq!(
        usize::from(CommentFixture::text_position(&after, text).0),
        row,
        "posting moved {text} on screen"
    );
}

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

#[test]
fn posting_a_question_does_not_move_the_diff_scroll() {
    let mut fixture = CommentFixture::new();
    fixture.key(Key::Last);
    fixture.key(Key::Char('a'));
    fixture
        .registry
        .publish(ui_events::TextPasted(
            (0..12)
                .map(|row| format!("Question row {row}."))
                .collect::<Vec<_>>()
                .join("\n"),
        ))
        .unwrap();
    fixture
        .registry
        .publish(DiffViewportChanged {
            width: 78,
            height: 10,
        })
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
    let before = fixture
        .component()
        .selected_document()
        .unwrap()
        .document
        .scroll;
    assert!(before > 0);
    fixture.key(Key::ControlEnter);
    let after = fixture
        .component()
        .selected_document()
        .unwrap()
        .document
        .scroll;
    assert_eq!(after, before);
}

#[test]
fn posting_a_question_keeps_visible_code_below_it_in_place() {
    let mut fixture = CommentFixture::new();
    let code = (1..=30)
        .map(|line| format!("Code row {line:02}"))
        .collect::<Vec<_>>();
    let rows = std::iter::once(DiffRow::Hunk {
        old_start: 1,
        old_count: 0,
        new_start: 1,
        new_count: 30,
    })
    .chain(code.iter().enumerate().map(|(index, text)| DiffRow::Add {
        new_line: u32::try_from(index + 1).unwrap(),
        text: format!("+{text}"),
    }))
    .collect();
    fixture
        .registry
        .publish(DiffContentLoaded {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            path: "src/lib.rs".into(),
            rows,
            old_content: Some(Vec::new()),
            new_content: Some(code.join("\n").into_bytes()),
        })
        .unwrap();
    fixture
        .registry
        .publish(DiffViewportChanged {
            width: 78,
            height: 10,
        })
        .unwrap();
    fixture.key(Key::First);
    for _ in 0..4 {
        fixture.key(Key::Down);
    }
    fixture.key(Key::Char('a'));
    assert!(fixture.component().comments.editing.is_some());
    fixture
        .registry
        .publish(ui_events::TextPasted(
            (0..12)
                .map(|row| format!("Question row {row}."))
                .collect::<Vec<_>>()
                .join("\n"),
        ))
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
    post_preserving_visible_line(&mut fixture, &code);
}

#[test]
fn posting_a_question_keeps_an_existing_visible_thread_in_place() {
    let mut fixture = CommentFixture::new();
    let existing = (0..20)
        .map(|row| format!("Existing row {row:02}."))
        .collect::<Vec<_>>();
    fixture.add(&existing.join("\n"));
    fixture.key(Key::Last);
    fixture.key(Key::Char('a'));
    fixture
        .registry
        .publish(ui_events::TextPasted("Another question".into()))
        .unwrap();
    fixture
        .registry
        .publish(DiffViewportChanged {
            width: 78,
            height: 10,
        })
        .unwrap();
    for delta in [isize::MAX, -8] {
        fixture
            .registry
            .dispatch_hovered_input(
                &EventEnvelope::new(PointerInput {
                    kind: PointerInputKind::Scroll(delta),
                    position: None,
                }),
                fixture.target,
            )
            .unwrap();
    }
    post_preserving_visible_line(&mut fixture, &existing);
}
