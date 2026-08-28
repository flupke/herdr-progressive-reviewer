use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Text;
use ratatui::widgets::{Clear, Paragraph, Widget, Wrap};

use crate::ReviewApp;
use crate::render::pane_block;

pub(super) struct PopupView<'a> {
    app: &'a ReviewApp,
    title: &'a str,
    content: Text<'a>,
    wrap: bool,
    scroll: u16,
}

impl<'a> PopupView<'a> {
    pub(super) fn new(app: &'a ReviewApp, title: &'a str, content: impl Into<Text<'a>>) -> Self {
        Self {
            app,
            title,
            content: content.into(),
            wrap: false,
            scroll: 0,
        }
    }

    pub(super) fn wrap(mut self) -> Self {
        self.wrap = true;
        self
    }

    pub(super) fn scroll(mut self, rows: u16) -> Self {
        self.scroll = rows;
        self
    }
}

impl Widget for PopupView<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        Clear.render(area, buffer);
        let paragraph = Paragraph::new(self.content)
            .block(pane_block(self.app, self.title, true))
            .scroll((self.scroll, 0));
        if self.wrap {
            paragraph.wrap(Wrap { trim: false }).render(area, buffer);
        } else {
            paragraph.render(area, buffer);
        }
    }
}
