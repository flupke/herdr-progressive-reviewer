use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Text;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap};
use ui_theme::Palette;

pub(super) fn centered_area(area: Rect, width: u16, height: u16) -> Rect {
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

pub(super) fn popup_block(title: &str, palette: Palette) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .title(format!(" {title} "))
        .border_style(Style::default().fg(palette.focus))
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
        .block(popup_block(title, palette))
        .scroll((scroll, 0));
    if wrap {
        paragraph.wrap(Wrap { trim: false }).render(area, buffer);
    } else {
        paragraph.render(area, buffer);
    }
}
