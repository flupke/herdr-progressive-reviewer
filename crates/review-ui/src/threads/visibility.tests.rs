use super::*;

#[test]
fn arriving_visible_replies_clear_without_selecting_or_focusing_the_conversation() {
    let mut ui = ThreadUi::new(110);
    ui.height = 70;
    ui.app.update(UserInput::Resize {
        width: ui.width,
        height: ui.height,
    });
    ui.key(Key::Char('t'));
    assert_eq!(ui.app.focus, ReviewPane::Navigation);
    ui.answer(
        0,
        "00000000-0000-4000-8000-000000000081",
        "First visible answer",
    );
    assert_eq!(ui.book.counts().unread, 1);
    assert!(ui.text().contains("First visible answer"));
    ui.present();
    assert_eq!(ui.book.counts().unread, 0);
    ui.answer(
        0,
        "00000000-0000-4000-8000-000000000082",
        "Another visible answer",
    );
    assert_eq!(ui.book.counts().unread, 1);
    ui.present();
    assert_eq!(ui.book.counts().unread, 0);
    assert_eq!(ui.app.focus, ReviewPane::Navigation);
    assert!(
        ui.present().is_empty(),
        "a persisted read must not be sent again"
    );
}

#[test]
fn long_replies_need_every_part_to_be_shown_and_jumping_to_the_end_is_insufficient() {
    let mut ui = ThreadUi::new(110);
    let answer = format!(
        "```\n{}\n```",
        (0..70)
            .map(|row| format!("answer line {row:02}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    ui.answer(0, "00000000-0000-4000-8000-000000000083", &answer);
    let later = MessageId::parse("00000000-0000-4000-8000-000000000085").unwrap();
    ui.answer(
        0,
        "00000000-0000-4000-8000-000000000085",
        "Short later answer",
    );
    ui.key(Key::Char('t'));
    ui.key(Key::Enter);
    assert_eq!(ui.book.counts().unread, 1, "opening is not reading");
    for _ in 0..8 {
        ui.key(Key::PageDown);
    }
    ui.present();
    assert!(ui.text().contains("answer line 69"));
    assert!(!ui.book.thread(&ui.ids[0]).unwrap().has_unread_reply(&later));
    assert_eq!(
        ui.book.counts().unread,
        1,
        "unseen middle rows must stay unread"
    );
    for _ in 0..8 {
        ui.key(Key::PageUp);
    }
    for _ in 0..110 {
        ui.present();
        ui.key(Key::Down);
    }
    assert_eq!(ui.book.counts().unread, 0);
}

#[test]
fn hidden_narrow_detail_and_failed_or_obscured_frames_do_not_mark_replies_read() {
    let mut ui = ThreadUi::new(48);
    ui.height = 70;
    ui.app.update(UserInput::Resize {
        width: ui.width,
        height: ui.height,
    });
    ui.answer(
        0,
        "00000000-0000-4000-8000-000000000084",
        "Answer under an overlay",
    );
    ui.key(Key::Char('t'));
    ui.present();
    assert_eq!(ui.book.counts().unread, 1);
    ui.key(Key::Enter);
    let mut buffer = ui.buffer();
    assert_eq!(
        ui.book.counts().unread,
        1,
        "rendering alone cannot acknowledge a failed draw"
    );
    let (column, row) =
        rendered_text_position(&ui.app, "Answer under", ui.width, ui.height).unwrap();
    buffer[(column, row)].set_symbol("X");
    ui.app.frame().diff.finish_reply_frame(&buffer);
    assert!(ui.app.publish(ui_events::FrameRendered).is_empty());
    assert_eq!(ui.book.counts().unread, 1);
    ui.present();
    assert_eq!(ui.book.counts().unread, 0);
}
