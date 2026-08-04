use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use unicode_width::UnicodeWidthStr;

use crate::ReviewApp;
use review_store::OutputTarget;

const CONTROLS: &str =
    "Tab focus · j/k move · l expand · v select · c message · Enter insert · Space review · q quit";
const OUTPUT_PREFIX: &str = "Output: ";
const AGENT_LABEL: &str = "[Active agent]";
const CLIPBOARD_LABEL: &str = "[Clipboard]";

pub(super) struct FooterView<'a>(pub(super) &'a ReviewApp);

impl FooterView<'_> {
    pub(super) fn output_target_at(column: u16) -> Option<OutputTarget> {
        let agent_start = u16::try_from(OUTPUT_PREFIX.width()).ok()?;
        let agent_end = agent_start + u16::try_from(AGENT_LABEL.width()).ok()?;
        let clipboard_start = agent_end + 1;
        let clipboard_end = clipboard_start + u16::try_from(CLIPBOARD_LABEL.width()).ok()?;
        if (agent_start..agent_end).contains(&column) {
            Some(OutputTarget::ActiveAgent)
        } else if (clipboard_start..clipboard_end).contains(&column) {
            Some(OutputTarget::Clipboard)
        } else {
            None
        }
    }

    fn target_style(&self, target: OutputTarget) -> Style {
        if self.0.output_target == target {
            Style::default()
                .fg(self.0.palette.focus)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(self.0.palette.dim)
        }
    }
}

impl Widget for FooterView<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        Paragraph::new(Line::from(vec![
            Span::raw(OUTPUT_PREFIX),
            Span::styled(AGENT_LABEL, self.target_style(OutputTarget::ActiveAgent)),
            Span::raw(" "),
            Span::styled(CLIPBOARD_LABEL, self.target_style(OutputTarget::Clipboard)),
            Span::raw(" · o toggle"),
        ]))
        .style(Style::default().fg(self.0.palette.text))
        .render(Rect::new(area.x, area.y, area.width, 1), buffer);
        let text = self.0.search.as_ref().map_or_else(
            || CONTROLS.to_owned(),
            |search| {
                if search.editing {
                    format!("/{} · Enter confirm · Esc clear", search.query)
                } else {
                    format!("/{} · n next · p previous · Esc clear", search.query)
                }
            },
        );
        Paragraph::new(text)
            .style(Style::default().fg(self.0.palette.text))
            .render(
                Rect::new(area.x, area.y.saturating_add(1), area.width, 1),
                buffer,
            );
    }
}
