//! Ratatui rendering for the complete review state.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::{Paragraph, Widget};

use crate::app::{Focus, PaneLayout, ReviewApp};
use crate::commit_message::CommitMessageView;
use crate::context_menu::ContextMenuView;
use crate::diff::DiffView;
use crate::files::FilesView;
use crate::footer::FooterView;
use crate::header::HeaderView;
use crate::hover::HoverView;

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
        if let Some(markdown) = &self.0.hover {
            HoverView(self.0, markdown).render(area, buffer);
        }
        if let Some(menu) = &self.0.context_menu {
            ContextMenuView(self.0, menu).render(area, buffer);
        }
        self.0
            .toasts
            .render(area, buffer, self.0.palette.focus, self.0.palette.deletion);
        if self.0.show_commit_message {
            CommitMessageView(self.0).render(area, buffer);
        }
    }
}
