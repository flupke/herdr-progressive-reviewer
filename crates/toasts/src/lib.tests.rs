use super::*;

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
