use ratatui::style::{Modifier, Style};
use ui_theme::Palette;

pub(super) enum ButtonStyle {
    Primary,
    Secondary,
}

impl ButtonStyle {
    pub(super) fn style(&self, palette: Palette) -> Style {
        let (foreground, background) = match self {
            Self::Primary => (palette.background, palette.insertion),
            Self::Secondary => (palette.text, palette.selection),
        };
        Style::default()
            .fg(foreground)
            .bg(background)
            .add_modifier(Modifier::BOLD)
    }
}
