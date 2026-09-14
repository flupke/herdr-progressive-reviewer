use super::{Filter, ThreadsComponent};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget},
};
use review_threads::{Resolution, ReviewThread};
use ui_theme::Palette;

impl ThreadsComponent {
    pub fn render(&self, area: Rect, buffer: &mut Buffer, palette: Palette, focused: bool) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if focused { palette.focus } else { palette.dim }));
        let inner = block.inner(area);
        block.render(area, buffer);
        let visible = self.visible();
        let search_rows = u16::try_from(self.search_rows()).unwrap_or_default();
        let cards = Rect::new(
            inner.x,
            inner.y.saturating_add(search_rows),
            inner.width,
            inner.height.saturating_sub(1 + search_rows),
        );
        if search_rows > 0 {
            Paragraph::new(format!(
                "/{}{}",
                self.query,
                if self.searching { "▏" } else { "" },
            ))
            .style(Style::default().fg(palette.dim))
            .render(
                Rect::new(
                    inner.x,
                    inner.y,
                    inner.width,
                    inner.height.saturating_sub(1).min(1),
                ),
                buffer,
            );
        }
        for (index, thread) in visible
            .into_iter()
            .skip(self.scroll)
            .take(self.page_rows())
            .enumerate()
        {
            let row = u16::try_from(index * Self::CARD_HEIGHT).unwrap_or(u16::MAX);
            let card = Rect::new(
                cards.x,
                cards.y.saturating_add(row),
                cards.width,
                u16::try_from(Self::CARD_HEIGHT).unwrap_or(u16::MAX),
            )
            .intersection(cards);
            self.render_card(thread, card, buffer, palette);
        }
        let filters = Filter::ALL
            .map(|filter| filter.label(self.filter))
            .join(" / ");
        let footer = Rect::new(
            inner.x,
            inner.bottom().saturating_sub(1),
            inner.width,
            u16::from(inner.height > 0),
        );
        Paragraph::new(filters)
            .style(Style::default().fg(palette.focus))
            .render(footer, buffer);
    }

    fn render_card(
        &self,
        thread: &ReviewThread,
        area: Rect,
        buffer: &mut Buffer,
        palette: Palette,
    ) {
        let selected = self.selected.as_ref() == Some(&thread.id);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if selected { palette.focus } else { palette.dim }));
        let content = block.inner(area);
        block.render(area, buffer);
        Paragraph::new(self.card(thread, palette)).render(content, buffer);
    }

    fn card(&self, thread: &ReviewThread, palette: Palette) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let selected = self.selected.as_ref() == Some(&thread.id);
        let style = if selected {
            Style::default()
                .fg(palette.focus)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(palette.text)
        };
        let attention = if thread.has_unread_replies() {
            "● New reply"
        } else if thread.resolution == Resolution::Resolved {
            "✓ Resolved"
        } else if thread.is_waiting() {
            "◷ Waiting"
        } else {
            "○ Open"
        };
        let question = thread.messages.first().map_or("", |message| {
            message.text.lines().next().unwrap_or_default()
        });
        lines.push(
            Line::raw(format!("{} {question}", if selected { "▸" } else { " " })).style(style),
        );
        lines.push(
            Line::raw(format!("  {}", thread.path())).style(Style::default().fg(palette.dim)),
        );
        let mut attention = if thread.has_unread_replies() {
            Line::from(vec![
                Span::raw("  "),
                Span::styled("●", Style::default().fg(palette.deletion)),
                Span::styled(" New reply", style),
            ])
        } else {
            Line::raw(format!("  {attention}")).style(style)
        };
        let file = self.file_for_thread(thread);
        if file.is_some_and(|file| !file.temporary && !file.review_state.status.needs_review()) {
            attention.spans.push(Span::styled(
                " · File reviewed",
                Style::default().fg(palette.dim),
            ));
        }
        lines.push(attention);
        let context = self
            .contexts
            .get(&thread.id)
            .copied()
            .unwrap_or(if file.is_some() {
                ui_events::ThreadContext::Original
            } else {
                ui_events::ThreadContext::OutsideDiff
            });
        lines.push(
            Line::raw(format!("  {}", context.label())).style(Style::default().fg(palette.dim)),
        );
        lines
    }
}
