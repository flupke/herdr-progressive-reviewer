use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Text};
use ui_shortcuts::{self as shortcuts, Key, NavigationShortcut, ShortcutCommand, ShortcutLookup};
use ui_theme::Palette;
use unicode_width::UnicodeWidthStr;

use crate::popup::{centered_area, render_popup};

const BORDER: u16 = 2;
const COLUMN_GAP: usize = 2;
const MINIMUM_DESCRIPTION_WIDTH: usize = 20;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct ShortcutHelpOverlay {
    scroll: u16,
}

impl ShortcutHelpOverlay {
    pub(super) fn open(&mut self) {
        self.scroll = 0;
    }

    pub(super) fn resize(&mut self, viewport: Rect) {
        self.scroll = self.scroll.min(Self::maximum_scroll(viewport));
    }

    pub(super) fn handle_key(&mut self, key: Key, viewport: Rect) -> bool {
        if shortcuts::closes_help(key) {
            return true;
        }
        match shortcuts::lookup(None, key) {
            ShortcutLookup::Command(ShortcutCommand::Navigation(NavigationShortcut::MoveDown)) => {
                self.scroll = self
                    .scroll
                    .saturating_add(1)
                    .min(Self::maximum_scroll(viewport));
            }
            ShortcutLookup::Command(ShortcutCommand::Navigation(NavigationShortcut::MoveUp)) => {
                self.scroll = self.scroll.saturating_sub(1);
            }
            _ => {}
        }
        false
    }

    pub(super) fn area(viewport: Rect) -> Rect {
        Self::layout(viewport, &HelpTable::new()).0
    }

    pub(super) fn render(&self, viewport: Rect, buffer: &mut Buffer, palette: Palette) {
        let (area, rows) = Self::layout(viewport, &HelpTable::new());
        render_popup(
            area,
            buffer,
            &format!(
                "Keyboard shortcuts · {} close",
                shortcuts::help_close_label()
            ),
            Text::from(rows.into_iter().map(Line::raw).collect::<Vec<_>>()),
            false,
            self.scroll,
            palette,
        );
    }

    fn maximum_scroll(viewport: Rect) -> u16 {
        let (area, rows) = Self::layout(viewport, &HelpTable::new());
        let visible = usize::from(area.height.saturating_sub(BORDER));
        u16::try_from(rows.len().saturating_sub(visible)).unwrap_or(u16::MAX)
    }

    /// Size the popup to its longest row, then wrap rows to the width it got.
    fn layout(viewport: Rect, table: &HelpTable) -> (Rect, Vec<String>) {
        let width = u16::try_from(table.natural_width())
            .unwrap_or(u16::MAX)
            .saturating_add(BORDER)
            // Leave a margin so a click outside can still dismiss the popup.
            .min(viewport.width.saturating_sub(2));
        let rows = table.rows(width.saturating_sub(BORDER));
        let height = u16::try_from(rows.len())
            .unwrap_or(u16::MAX)
            .saturating_add(BORDER)
            .min(viewport.height);
        (centered_area(viewport, width, height), rows)
    }
}

/// Shortcut keys and descriptions laid out as two columns.
struct HelpTable {
    lines: Vec<(String, &'static str)>,
    key_width: usize,
}

impl HelpTable {
    fn new() -> Self {
        let lines = shortcuts::help_lines().collect::<Vec<_>>();
        let key_width = lines
            .iter()
            .map(|(keys, _)| keys.width())
            .max()
            .unwrap_or_default();
        Self { lines, key_width }
    }

    fn natural_width(&self) -> usize {
        let description_width = self
            .lines
            .iter()
            .map(|(_, description)| description.width())
            .max()
            .unwrap_or_default();
        self.key_width + COLUMN_GAP + description_width
    }

    /// Wrap descriptions under their column so every word stays readable.
    ///
    /// When the description column would be too narrow, each key gets its own
    /// row and its description wraps below it.
    fn rows(&self, width: u16) -> Vec<String> {
        let width = usize::from(width);
        let key_width = self.key_width;
        let column_width = width.saturating_sub(key_width + COLUMN_GAP);
        let stacked = column_width < MINIMUM_DESCRIPTION_WIDTH;
        let (indent, description_width) = if stacked {
            (COLUMN_GAP, width.saturating_sub(COLUMN_GAP))
        } else {
            (key_width + COLUMN_GAP, column_width)
        };
        let mut rows = Vec::new();
        for (keys, description) in &self.lines {
            let mut label = keys.as_str();
            if stacked {
                rows.push(label.to_owned());
                label = "";
            }
            for part in wrap_words(description, description_width.max(1)) {
                let padding = " ".repeat(indent.saturating_sub(label.width()));
                rows.push(format!("{label}{padding}{part}"));
                label = "";
            }
        }
        rows
    }
}

fn wrap_words(text: &str, width: usize) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    for word in text.split(' ') {
        if !current.is_empty() && current.width() + 1 + word.width() > width {
            parts.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    parts.push(current);
    parts
}

#[cfg(test)]
#[path = "shortcut_help.tests.rs"]
mod tests;
