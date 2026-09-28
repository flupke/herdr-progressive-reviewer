//! Confirmation input and rendering shared by application actions.

use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    text::Text,
};
use ui_actions::Action;
use ui_controls::{ActionButton, ButtonTone, label_width};
use ui_events::{PointerInput, PointerInputKind};
use ui_shortcuts::Key;
use ui_theme::Palette;

use super::popup::{centered_area, render_popup};

pub(super) struct ConfirmationOverlay {
    pub(super) title: &'static str,
    pub(super) question: &'static str,
    pub(super) action: Action,
}

pub(super) enum Decision {
    Confirm,
    Cancel,
}

impl ConfirmationOverlay {
    pub(super) fn area(viewport: Rect) -> Rect {
        centered_area(viewport, viewport.width.min(48), viewport.height.min(6))
    }

    fn buttons(area: Rect) -> [(Decision, ActionButton, Rect); 2] {
        let width = area.width.saturating_sub(2);
        let row = area.bottom().saturating_sub(2).max(area.y);
        let confirm = ActionButton::new("[y] Yes", ButtonTone::Primary);
        let cancel = ActionButton::new("[n] No", ButtonTone::Secondary);
        let confirm_width = label_width(&confirm.text());
        let cancel_width = label_width(&cancel.text());
        let gap = width
            .saturating_sub(confirm_width.saturating_add(cancel_width))
            .min(4);
        let group_width = confirm_width
            .saturating_add(gap)
            .saturating_add(cancel_width);
        let start = area
            .x
            .saturating_add(1)
            .saturating_add(width.saturating_sub(group_width) / 2);
        let left = Rect::new(start, row, width.min(confirm_width), 1);
        let right_x = left.right().saturating_add(gap);
        let right = Rect::new(
            right_x,
            row,
            area.right()
                .saturating_sub(1)
                .saturating_sub(right_x)
                .min(cancel_width),
            1,
        );
        [
            (Decision::Confirm, confirm, left),
            (Decision::Cancel, cancel, right),
        ]
    }

    pub(super) fn render(&self, viewport: Rect, buffer: &mut Buffer, palette: Palette) {
        let area = Self::area(viewport);
        render_popup(
            area,
            buffer,
            self.title,
            Text::raw(self.question).centered(),
            true,
            0,
            palette,
        );
        if area.height >= 4 && area.width >= 2 {
            for (_, button, area) in Self::buttons(area) {
                button.render(area, buffer, palette);
            }
        }
    }

    pub(super) fn key(key: Key) -> Option<Decision> {
        match key {
            Key::Char('y' | 'Y') => Some(Decision::Confirm),
            Key::Char('n' | 'N') | Key::Escape => Some(Decision::Cancel),
            _ => None,
        }
    }

    pub(super) fn pointer(viewport: Rect, input: PointerInput) -> Option<Decision> {
        let area = Self::area(viewport);
        if input.kind != PointerInputKind::Click || area.height < 4 || area.width < 2 {
            return None;
        }
        let position = input.position?;
        let position = Position::new(position.terminal_column, position.terminal_row);
        Self::buttons(area)
            .into_iter()
            .find_map(|(decision, _, area)| area.contains(position).then_some(decision))
    }
}
