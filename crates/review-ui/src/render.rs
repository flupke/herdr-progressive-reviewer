use ratatui::style::Style;
use ratatui::widgets::{Block, Borders};

use crate::app::ReviewApp;

pub(super) fn pane_block<'a>(app: &ReviewApp, title: &'a str, focused: bool) -> Block<'a> {
    let style = if focused {
        Style::default().fg(app.palette.focus)
    } else {
        Style::default().fg(app.palette.dim)
    };
    let suffix = if focused { " (focus)" } else { "" };
    Block::default()
        .borders(Borders::ALL)
        .title(format!(" {title}{suffix} "))
        .border_style(style)
}

pub(super) fn shorten(value: &str, width: usize) -> String {
    let length = value.chars().count();
    if length <= width {
        return value.to_owned();
    }
    if width <= 3 {
        return ".".repeat(width);
    }
    let left = (width - 1) / 2;
    let right = width - left - 1;
    format!(
        "{}…{}",
        value.chars().take(left).collect::<String>(),
        value.chars().skip(length - right).collect::<String>()
    )
}
