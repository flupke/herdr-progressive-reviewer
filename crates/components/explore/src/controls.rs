use super::Control;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    widgets::{Paragraph, Widget},
};
use ui_controls::{ActionButton, ButtonTone, NavigationLink};
use ui_theme::Palette;

#[derive(Clone)]
pub(super) enum ControlVisual {
    Text(String),
    Action(ActionButton),
    Link(NavigationLink),
}

impl ControlVisual {
    fn text(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Action(button) => button.text(),
            Self::Link(link) => link.text(),
        }
    }
}

/// Control labels share row measurement in pinned and scrolling layouts.
#[derive(Clone)]
pub(super) struct Button {
    pub(super) text: String,
    pub(super) column: u16,
    pub(super) control: Option<Control>,
    visual: ControlVisual,
}

impl Button {
    pub(super) fn render_row(
        buttons: &[Self],
        visible: Rect,
        buffer: &mut Buffer,
        palette: Palette,
    ) {
        for button in buttons {
            let width = visible.width.saturating_sub(button.column);
            button.render(
                Rect::new(
                    visible.x + button.column.min(visible.width),
                    visible.y,
                    width,
                    1,
                ),
                buffer,
                palette,
            );
        }
    }

    pub(super) fn wrap(
        width: u16,
        labels: impl IntoIterator<Item = (ControlVisual, Option<Control>)>,
    ) -> Vec<Vec<Self>> {
        let mut rows = Vec::new();
        let mut row = Vec::new();
        let mut column: u16 = 0;
        for (visual, control) in labels {
            let text = visual.text();
            let length = u16::try_from(text.len()).unwrap_or(u16::MAX);
            if column > 0 && column.saturating_add(length) > width {
                rows.push(std::mem::take(&mut row));
                column = 0;
            }
            row.push(Self {
                text,
                column,
                control,
                visual,
            });
            column = column.saturating_add(length).saturating_add(1);
        }
        if !row.is_empty() {
            rows.push(row);
        }
        rows
    }

    pub(super) fn render(&self, area: Rect, buffer: &mut Buffer, palette: Palette) {
        match &self.visual {
            ControlVisual::Action(button) => button.render(area, buffer, palette),
            ControlVisual::Link(link) => link.render(area, buffer, palette),
            ControlVisual::Text(text) => Paragraph::new(text.as_str())
                .style(if self.control.is_some() {
                    NavigationLink::style(palette)
                } else {
                    Style::default().fg(palette.text)
                })
                .render(area, buffer),
        }
    }
}

impl Control {
    pub(super) fn visual(self, label: String) -> ControlVisual {
        if matches!(
            self,
            Self::Start
                | Self::NewImplementation
                | Self::Send
                | Self::Defer
                | Self::RequireReview(_)
                | Self::Cancel
                | Self::Retry
                | Self::Implement
                | Self::CancelImplementation
        ) {
            let tone = if matches!(self, Self::Start | Self::Send | Self::Implement) {
                ButtonTone::Primary
            } else {
                ButtonTone::Secondary
            };
            ControlVisual::Action(ActionButton::new(label, tone))
        } else {
            ControlVisual::Link(NavigationLink::new(label))
        }
    }
}
