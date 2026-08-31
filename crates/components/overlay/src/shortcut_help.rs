use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Text};
use ui_shortcuts::{self as shortcuts, Key, NavigationShortcut, ShortcutCommand, ShortcutLookup};
use ui_theme::Palette;

use crate::popup::{centered_area, render_popup};

const HELP_WIDTH: u16 = 72;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct ShortcutHelpOverlay {
    scroll: u16,
}

impl ShortcutHelpOverlay {
    pub(super) fn open(&mut self) {
        self.scroll = 0;
    }

    pub(super) fn resize(&mut self, terminal_height: u16) {
        self.scroll = self.scroll.min(Self::maximum_scroll(terminal_height));
    }

    pub(super) fn handle_key(&mut self, key: Key, terminal_height: u16) -> bool {
        if shortcuts::closes_help(key) {
            return true;
        }
        match shortcuts::lookup(None, key) {
            ShortcutLookup::Command(ShortcutCommand::Navigation(NavigationShortcut::MoveDown)) => {
                self.scroll = self
                    .scroll
                    .saturating_add(1)
                    .min(Self::maximum_scroll(terminal_height));
            }
            ShortcutLookup::Command(ShortcutCommand::Navigation(NavigationShortcut::MoveUp)) => {
                self.scroll = self.scroll.saturating_sub(1);
            }
            _ => {}
        }
        false
    }

    pub(super) fn area(area: Rect) -> Rect {
        let width = HELP_WIDTH.min(area.width);
        let height = u16::try_from(shortcuts::help_line_count())
            .unwrap_or(u16::MAX)
            .saturating_add(2)
            .min(area.height);
        centered_area(area, width, height)
    }

    pub(super) fn render(&self, area: Rect, buffer: &mut Buffer, palette: Palette) {
        let shortcuts = shortcuts::help_lines().collect::<Vec<_>>();
        let key_width = shortcuts
            .iter()
            .map(|(keys, _)| keys.len())
            .max()
            .unwrap_or_default();
        let lines = shortcuts
            .into_iter()
            .map(|(keys, description)| Line::raw(format!("{keys:<key_width$}  {description}")))
            .collect::<Vec<_>>();
        render_popup(
            Self::area(area),
            buffer,
            &format!(
                "Keyboard shortcuts · {} close",
                shortcuts::help_close_label()
            ),
            Text::from(lines),
            false,
            self.scroll,
            palette,
        );
    }

    fn maximum_scroll(terminal_height: u16) -> u16 {
        let content_height = shortcuts::help_line_count();
        let popup_height = content_height
            .saturating_add(2)
            .min(usize::from(terminal_height));
        u16::try_from(content_height.saturating_sub(popup_height.saturating_sub(2)))
            .unwrap_or(u16::MAX)
    }
}
