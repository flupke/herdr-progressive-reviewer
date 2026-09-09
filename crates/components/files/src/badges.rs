use ratatui::{
    style::Style,
    text::{Line, Span},
};
use review_threads::ThreadCounts;
use ui_theme::Palette;

pub(super) struct FileBadges {
    pub(super) guide: bool,
    pub(super) threads: ThreadCounts,
}

impl FileBadges {
    pub(super) fn line(self, palette: Palette) -> Line<'static> {
        let mut spans = Vec::new();
        if self.guide {
            spans.push(Span::styled(" 📄", Style::default().fg(palette.guide)));
        }
        if self.threads.open > 0 {
            spans.push(Span::styled(" 💬", Style::default().fg(palette.focus)));
        }
        if self.threads.unread > 0 {
            spans.push(Span::styled(" ●", Style::default().fg(palette.deletion)));
        }
        Line::from(spans)
    }
}
