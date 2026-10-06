use super::*;

#[test]
fn timer_changes_only_at_toast_appearance_and_expiration() {
    let mut toasts = ToastState::default();
    let initial = Instant::now();
    assert!(!toasts.changes_between(initial, initial + Duration::from_secs(10)));
    toasts.push_at("saved", ToastKind::Info, initial);
    let expires = initial + Duration::from_secs(3);
    let long = toasts.start_long_toast_at("waiting for the server", initial);
    let appears = initial + LONG_TOAST_DELAY;
    assert!(!toasts.changes_between(
        initial,
        appears.checked_sub(Duration::from_nanos(1)).unwrap()
    ));
    assert!(toasts.changes_between(initial, appears));
    assert!(!toasts.changes_between(
        appears,
        expires.checked_sub(Duration::from_nanos(1)).unwrap()
    ));
    assert!(toasts.changes_between(appears, expires));
    toasts.expire(expires);
    assert!(!toasts.changes_between(expires, expires + Duration::from_secs(10)));
    toasts.finish_toast(long);
    assert!(!toasts.changes_between(expires, expires + Duration::from_secs(10)));
}

#[test]
fn pushed_toast_expires_after_its_kind_duration() {
    let now = Instant::now();
    let mut toasts = ToastState::default();
    toasts.push_at("message", ToastKind::Info, now);
    toasts.push_at("failure", ToastKind::Error, now);

    assert_eq!(toasts.toasts[0].expires, now + Duration::from_secs(3));
    assert_eq!(toasts.toasts[1].expires, now + Duration::from_secs(6));
}

#[test]
fn expiration_removes_a_toast_at_its_exact_deadline() {
    let now = Instant::now();
    let mut toasts = ToastState::default();
    toasts.push_at("message", ToastKind::Info, now);
    let deadline = now + Duration::from_secs(3);

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

    toasts.render(buffer.area, &mut buffer, palette());

    let content = buffer
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect::<String>();
    assert!(!content.contains("oldest"));
    assert!(content.contains("middle"));
    assert!(content.contains("newest"));
    assert!(buffer.content.iter().any(|cell| cell.fg == palette().focus));
    assert!(
        buffer
            .content
            .iter()
            .any(|cell| cell.fg == palette().deletion)
    );
}

#[test]
fn toasts_stack_delay_and_expire_independently() {
    let now = Instant::now();
    let mut toasts = ToastState::default();
    toasts.push_at("first", ToastKind::Info, now);
    toasts.push_at("second", ToastKind::Error, now);
    let long = toasts.start_long_toast_at("long", now);
    let mut buffer = Buffer::empty(Rect::new(0, 0, 80, 12));

    toasts.render_at(buffer.area, &mut buffer, palette(), now);
    let content = buffer
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect::<String>();
    assert!(content.contains("first"));
    assert!(content.contains("second"));
    assert!(!content.contains("long"));

    toasts.render_at(buffer.area, &mut buffer, palette(), now + LONG_TOAST_DELAY);
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

#[test]
fn a_long_toast_appears_after_the_delay_it_was_given() {
    let now = Instant::now();
    let mut toasts = ToastState::with_long_toast_delay(Duration::ZERO);
    toasts.start_long_toast_at("at once", now);
    let mut buffer = Buffer::empty(Rect::new(0, 0, 40, 3));

    toasts.render_at(buffer.area, &mut buffer, palette(), now);

    assert!(
        buffer
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect::<String>()
            .contains("at once")
    );
}

fn palette() -> Palette {
    ui_theme::Theme::default().palette
}
