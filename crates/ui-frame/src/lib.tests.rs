use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};
use ratatui::widgets::Widget;
use ui_theme::Theme;

use super::Frame;

fn draw(frame: Frame, title: &str) -> Buffer {
    let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 3));
    frame
        .block(Theme::default().palette, title)
        .render(buffer.area, &mut buffer);
    buffer
}

fn row(buffer: &Buffer, y: u16) -> String {
    (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol())
        .collect()
}

#[test]
fn frames_have_rounded_corners_and_a_padded_title() {
    let buffer = draw(Frame::Popup, "Help");

    assert_eq!(row(&buffer, 0), "╭ Help ────╮");
    assert_eq!(row(&buffer, 2), "╰──────────╯");
}

#[test]
fn an_empty_title_leaves_the_border_whole() {
    assert_eq!(row(&draw(Frame::Popup, ""), 0), "╭──────────╮");
}

#[test]
fn only_the_frame_with_attention_takes_the_accent() {
    let palette = Theme::default().palette;
    let corner = |frame| draw(frame, "Diff")[(0, 0)].fg;
    let title = |frame| draw(frame, "Diff")[(2, 0)].style();

    assert_eq!(corner(Frame::Pane { focused: true }), palette.focus);
    assert_eq!(corner(Frame::Popup), palette.focus);
    assert_eq!(corner(Frame::Card { selected: true }), palette.focus);
    assert_eq!(corner(Frame::Pane { focused: false }), palette.border);
    assert_eq!(corner(Frame::Card { selected: false }), palette.border);
    assert_eq!(title(Frame::Pane { focused: true }).fg, Some(palette.text));
    assert!(
        title(Frame::Pane { focused: true })
            .add_modifier
            .contains(Modifier::BOLD)
    );
    assert_eq!(title(Frame::Pane { focused: false }).fg, Some(palette.dim));
}

#[test]
fn a_notice_is_framed_and_titled_in_its_own_color() {
    let buffer = draw(Frame::Notice(Color::Red), "Saved");

    assert_eq!(buffer[(0, 0)].fg, Color::Red);
    assert_eq!(buffer[(2, 0)].fg, Color::Red);
}
