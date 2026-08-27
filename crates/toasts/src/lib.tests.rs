use super::*;

#[test]
fn toast_kinds_have_distinct_display_durations() {
    assert_eq!(ToastKind::Info.duration(), Duration::from_secs(3));
    assert_eq!(ToastKind::Error.duration(), Duration::from_secs(6));
}

#[test]
fn pushed_toast_expires_after_its_kind_duration() {
    let before_push = Instant::now();
    let mut toasts = ToastState::default();
    toasts.push("message", ToastKind::Info);
    let after_push = Instant::now();

    assert!(toasts.toasts[0].expires >= before_push + Duration::from_secs(3));
    assert!(toasts.toasts[0].expires <= after_push + Duration::from_secs(3));
}

#[test]
fn expiration_removes_a_toast_at_its_exact_deadline() {
    let mut toasts = ToastState::default();
    toasts.push("message", ToastKind::Info);
    let deadline = toasts.toasts[0].expires;

    toasts.expire(deadline.checked_sub(Duration::from_nanos(1)).unwrap());
    assert_eq!(toasts.toasts.len(), 1);
    toasts.expire(deadline);
    assert!(toasts.toasts.is_empty());
}

#[test]
fn public_render_draws_only_the_number_of_toasts_that_fit() {
    let mut toasts = ToastState::default();
    toasts.push("oldest", ToastKind::Info);
    toasts.push("middle", ToastKind::Info);
    toasts.push("newest", ToastKind::Error);
    let mut buffer = Buffer::empty(Rect::new(0, 0, 40, 6));

    toasts.render(buffer.area, &mut buffer, Color::Green, Color::Red);

    let content = buffer
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect::<String>();
    assert!(!content.contains("oldest"));
    assert!(content.contains("middle"));
    assert!(content.contains("newest"));
    assert!(buffer.content.iter().any(|cell| cell.fg == Color::Green));
    assert!(buffer.content.iter().any(|cell| cell.fg == Color::Red));
}

#[test]
fn toasts_stack_delay_and_expire_independently() {
    let now = Instant::now();
    let mut toasts = ToastState::default();
    toasts.push("first", ToastKind::Info);
    toasts.push("second", ToastKind::Error);
    let long = toasts.start_long_toast("long");
    let mut buffer = Buffer::empty(Rect::new(0, 0, 80, 12));

    toasts.render_at(buffer.area, &mut buffer, Color::Green, Color::Red, now);
    let content = buffer
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect::<String>();
    assert!(content.contains("first"));
    assert!(content.contains("second"));
    assert!(!content.contains("long"));

    toasts.long_toasts.front_mut().unwrap().started = now.checked_sub(LONG_TOAST_DELAY).unwrap();
    toasts.render_at(buffer.area, &mut buffer, Color::Green, Color::Red, now);
    assert!(
        buffer
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect::<String>()
            .contains("long")
    );

    toasts.finish_toast(long);
    toasts.expire(now + Duration::from_secs(7));
    assert!(toasts.toasts.is_empty());
    assert!(toasts.long_toasts.is_empty());
}
