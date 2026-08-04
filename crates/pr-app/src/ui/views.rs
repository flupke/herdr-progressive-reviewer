//! Ratatui rendering for the review state.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use super::{Focus, PaneLayout, ReviewApp};
pub(super) use commit_message::CommitMessageView;
use diff::DiffView;
use files::FilesView;
use footer::FooterView;
use header::HeaderView;

mod commit_message;
mod diff;
mod files;
mod footer;
mod header;

const MIN_WIDTH: u16 = 40;
const MIN_HEIGHT: u16 = 6;

/// A side-effect-free view of [`ReviewApp`].
pub struct ReviewView<'a>(pub(super) &'a ReviewApp);

impl Widget for ReviewView<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
            Paragraph::new("Terminal is too small\nMinimum: 40x6\nq quit").render(area, buffer);
            return;
        }

        let layout = PaneLayout::new(area.width, area.height, self.0.file_width);
        let header = Rect::new(area.x, area.y, area.width, 1);
        let body = Rect::new(area.x, area.y + 1, area.width, layout.body_height());
        let footer = Rect::new(
            area.x,
            area.bottom() - layout.footer_height,
            area.width,
            layout.footer_height,
        );
        HeaderView(self.0).render(header, buffer);
        if layout.is_wide() {
            let file_width = layout.file_width;
            FilesView(self.0).render(Rect::new(body.x, body.y, file_width, body.height), buffer);
            DiffView(self.0).render(
                Rect::new(
                    body.x + file_width,
                    body.y,
                    body.width - file_width,
                    body.height,
                ),
                buffer,
            );
        } else {
            match self.0.focus {
                Focus::Files => FilesView(self.0).render(body, buffer),
                Focus::Diff => DiffView(self.0).render(body, buffer),
            }
        }
        FooterView(self.0).render(footer, buffer);
        if self.0.show_commit_message {
            CommitMessageView(self.0).render(area, buffer);
        }
    }
}

fn pane_block<'a>(app: &ReviewApp, title: &'a str, focused: bool) -> Block<'a> {
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

fn shorten(value: &str, width: usize) -> String {
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
