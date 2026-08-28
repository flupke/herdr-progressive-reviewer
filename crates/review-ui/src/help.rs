use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Widget;

use crate::ReviewApp;
use crate::popup::PopupView;
use crate::shortcuts::{SHORTCUTS, ShortcutDefinition, help_close_label};

const HELP_WIDTH: u16 = 72;

pub(super) struct ShortcutHelpView<'a>(pub(super) &'a ReviewApp);

impl ShortcutHelpView<'_> {
    pub(super) fn area(area: Rect) -> Rect {
        let width = HELP_WIDTH.min(area.width);
        let content_height = visible_shortcut_count();
        let height = u16::try_from(content_height)
            .unwrap_or(u16::MAX)
            .saturating_add(2)
            .min(area.height);
        Rect::new(
            area.x + area.width.saturating_sub(width) / 2,
            area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        )
    }

    pub(super) fn maximum_scroll(terminal_height: u16) -> u16 {
        let content_height = visible_shortcut_count();
        let popup_height = content_height
            .saturating_add(2)
            .min(usize::from(terminal_height));
        u16::try_from(content_height.saturating_sub(popup_height.saturating_sub(2)))
            .unwrap_or(u16::MAX)
    }
}

fn visible_shortcut_count() -> usize {
    SHORTCUTS
        .iter()
        .filter(|shortcut| shortcut.description.is_some())
        .count()
}

impl Widget for ShortcutHelpView<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let popup = Self::area(area);
        let shortcuts = SHORTCUTS
            .iter()
            .filter_map(ShortcutDefinition::help_line)
            .collect::<Vec<_>>();
        let key_width = shortcuts
            .iter()
            .map(|(keys, _)| keys.len())
            .max()
            .unwrap_or_default();
        let lines = shortcuts
            .into_iter()
            .map(|(keys, description)| Line::raw(format!("{keys:<key_width$}  {description}")));
        let title = format!("Keyboard shortcuts · {} close", help_close_label());
        PopupView::new(self.0, &title, lines.collect::<Vec<_>>())
            .scroll(self.0.shortcut_help_scroll)
            .render(popup, buffer);
    }
}
