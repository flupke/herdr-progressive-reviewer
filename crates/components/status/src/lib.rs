//! Repository header and application status line.

use component_core::{AnyInput, Component, ComponentSubscriptions, EventPublisher, InputScope};
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use ui_actions::Action;
use ui_events::{
    CommitMessageToggleRequested, FilesOverviewChanged, PointerInput, PointerInputKind,
    RepositoryMetadataChanged, ReviewGuideStatusChanged, SearchStatusChanged,
};
use ui_theme::Palette;
use unicode_width::UnicodeWidthStr;

const GUIDE_SPINNER: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
const MIN_TERMINAL_WIDTH: u16 = 40;
const MIN_TERMINAL_HEIGHT: u16 = 6;

/// State and behavior for the repository header and status line.
pub struct StatusComponent {
    events: EventPublisher,
    description: String,
    overview: FilesOverviewChanged,
    search: SearchStatusChanged,
    guide_spinner_frame: Option<usize>,
}

impl StatusComponent {
    pub fn new(events: EventPublisher) -> Self {
        Self {
            events,
            description: String::new(),
            overview: FilesOverviewChanged::default(),
            search: SearchStatusChanged::default(),
            guide_spinner_frame: None,
        }
    }

    /// Render the complete small-terminal state when the viewport is too small.
    pub fn render_terminal_too_small(&self, area: Rect, buffer: &mut Buffer) -> bool {
        if area.width >= MIN_TERMINAL_WIDTH && area.height >= MIN_TERMINAL_HEIGHT {
            return false;
        }
        Paragraph::new("Terminal is too small\nMinimum: 40x6\nq quit").render(area, buffer);
        true
    }

    pub fn render_header(&self, area: Rect, buffer: &mut Buffer, palette: Palette) {
        Paragraph::new(format!(" {}", self.commit_title()))
            .style(Style::default().fg(palette.text))
            .render(area, buffer);
        let summary = format!(
            " - {}/{} reviewed ",
            self.overview.reviewed, self.overview.total
        );
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!("+{}", self.overview.lines_added),
                Style::default().fg(palette.insertion),
            ),
            Span::raw(" "),
            Span::styled(
                format!("-{}", self.overview.lines_removed),
                Style::default().fg(palette.deletion),
            ),
            Span::raw(summary),
        ]))
        .alignment(Alignment::Right)
        .style(Style::default().fg(palette.text))
        .render(area, buffer);
    }

    pub fn render_footer(&self, area: Rect, buffer: &mut Buffer, palette: Palette) {
        if let Some(query) = &self.search.query {
            Paragraph::new(format!("/{query}"))
                .style(Style::default().fg(palette.text))
                .render(area, buffer);
            Paragraph::new(format!(
                "[{}/{}]",
                self.search.current_match, self.search.total_matches
            ))
            .alignment(Alignment::Right)
            .style(Style::default().fg(palette.text))
            .render(area, buffer);
            return;
        }

        let guide_status = self.guide_spinner_frame.map(|frame| {
            format!(
                "{} Generating guide",
                GUIDE_SPINNER[frame % GUIDE_SPINNER.len()]
            )
        });
        let status_width = guide_status
            .as_deref()
            .map_or(0, |status| status.width().saturating_add(1));
        let status = Line::raw("? help");
        Paragraph::new(status)
            .style(Style::default().fg(palette.text))
            .render(
                Rect::new(
                    area.x,
                    area.y,
                    area.width
                        .saturating_sub(u16::try_from(status_width).unwrap_or(u16::MAX)),
                    1,
                ),
                buffer,
            );
        if let Some(guide_status) = guide_status {
            Paragraph::new(Span::styled(
                guide_status,
                Style::default().fg(palette.guide),
            ))
            .alignment(Alignment::Right)
            .render(area, buffer);
        }
    }

    fn repository_changed(&mut self, event: &RepositoryMetadataChanged) {
        self.description.clone_from(&event.description);
    }

    fn overview_changed(&mut self, event: &FilesOverviewChanged) {
        self.overview = *event;
    }

    fn search_changed(&mut self, event: &SearchStatusChanged) {
        self.search.clone_from(event);
    }

    fn guide_status_changed(&mut self, event: &ReviewGuideStatusChanged) {
        self.guide_spinner_frame = event.generating.then_some(0);
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn tick(&mut self, _event: &ui_events::AnimationTick) {
        if let Some(frame) = &mut self.guide_spinner_frame {
            *frame = frame.saturating_add(1);
        }
    }

    fn pointer_input(&mut self, input: PointerInput) -> Vec<Action> {
        if !matches!(input.kind, PointerInputKind::Click { .. }) {
            return Vec::new();
        }
        let Some(position) = input.position else {
            return Vec::new();
        };
        match position.terminal_row {
            0 if position.terminal_column > 0
                && usize::from(position.terminal_column) <= self.commit_title().width() =>
            {
                self.events.publish(CommitMessageToggleRequested);
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn commit_title(&self) -> &str {
        self.description
            .lines()
            .next()
            .unwrap_or("(no description set)")
    }
}

impl Component<Action> for StatusComponent {
    fn register_subscriptions(subscriptions: &mut ComponentSubscriptions<'_, Self, Action>) {
        subscriptions.subscribe(Self::repository_changed);
        subscriptions.subscribe(Self::overview_changed);
        subscriptions.subscribe(Self::search_changed);
        subscriptions.subscribe(Self::guide_status_changed);
        subscriptions.subscribe(Self::tick);
        subscriptions.subscribe_input(InputScope::Hovered, AnyInput, Self::pointer_input);
    }
}

#[cfg(test)]
#[path = "lib.tests.rs"]
mod tests;
