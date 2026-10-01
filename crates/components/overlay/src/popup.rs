use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Text;
use ratatui::widgets::{Clear, Paragraph, Widget, Wrap};
use ui_frame::Frame;
use ui_theme::Palette;

pub(super) fn centered_area(area: Rect, width: u16, height: u16) -> Rect {
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

pub(super) fn render_popup(
    area: Rect,
    buffer: &mut Buffer,
    title: &str,
    content: Text<'_>,
    wrap: bool,
    scroll: u16,
    palette: Palette,
) {
    Clear.render(area, buffer);
    let paragraph = Paragraph::new(content)
        .block(Frame::Popup.block(palette, title))
        .scroll((scroll, 0));
    if wrap {
        paragraph.wrap(Wrap { trim: false }).render(area, buffer);
    } else {
        paragraph.render(area, buffer);
    }
}
