//! Repository header and application status line.

use ansi_to_tui::IntoText;
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
    display_id: Line<'static>,
    overview: FilesOverviewChanged,
    search: SearchStatusChanged,
    guide_spinner_frame: Option<usize>,
}

impl StatusComponent {
    pub fn is_animating(&self) -> bool {
        self.guide_spinner_frame.is_some()
    }

    pub fn new(events: EventPublisher) -> Self {
        Self {
            events,
            description: String::new(),
            display_id: Line::default(),
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
        let summary = format!(
            " - {}/{} reviewed ",
            self.overview.reviewed, self.overview.total
        );
        let summary = Line::from(vec![
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
        ]);
        let summary_width = u16::try_from(summary.width())
            .unwrap_or(u16::MAX)
            .min(area.width);
        let title_width = area.width.saturating_sub(summary_width.saturating_add(1));
        let mut title = Line::from(" ");
        title.spans.extend(self.display_id.spans.iter().cloned());
        title
            .spans
            .push(Span::raw(format!(" {}", self.commit_title())));
        Paragraph::new(title)
            .style(Style::default().fg(palette.text))
            .render(Rect::new(area.x, area.y, title_width, 1), buffer);
        Paragraph::new(summary)
            .alignment(Alignment::Right)
            .style(Style::default().fg(palette.text))
            .render(
                Rect::new(
                    area.right().saturating_sub(summary_width),
                    area.y,
                    summary_width,
                    area.height,
                ),
                buffer,
            );
    }

    pub fn render_footer(&self, area: Rect, buffer: &mut Buffer, palette: Palette) {
        let mut statuses = Vec::new();
        if let Some(frame) = self.guide_spinner_frame {
            statuses.push(Span::styled(
                format!(
                    "{} Generating guide",
                    GUIDE_SPINNER[frame % GUIDE_SPINNER.len()]
                ),
                Style::default().fg(palette.guide),
            ));
        }
        if self.search.query.is_some() {
            statuses.push(Span::raw(format!(
                "[{}/{}]",
                self.search.current_match, self.search.total_matches
            )));
        }
        let status = Line::from(
            statuses
                .into_iter()
                .enumerate()
                .flat_map(|(index, span)| {
                    (index > 0)
                        .then_some(Span::raw(" · "))
                        .into_iter()
                        .chain([span])
                })
                .collect::<Vec<_>>(),
        );
        let width = u16::try_from(status.width())
            .unwrap_or(u16::MAX)
            .min(area.width);
        let left = self
            .search
            .query
            .as_ref()
            .map_or_else(|| "? help".to_owned(), |query| format!("/{query}"));
        Paragraph::new(left)
            .style(Style::default().fg(palette.text))
            .render(
                Rect::new(
                    area.x,
                    area.y,
                    area.width.saturating_sub(width.saturating_add(1)),
                    1,
                ),
                buffer,
            );
        Paragraph::new(status)
            .style(Style::default().fg(palette.text))
            .alignment(Alignment::Right)
            .render(
                Rect::new(area.right().saturating_sub(width), area.y, width, 1),
                buffer,
            );
    }

    fn repository_changed(&mut self, event: &RepositoryMetadataChanged) {
        self.description.clone_from(&event.description);
        self.display_id = event
            .display_id
            .as_bytes()
            .into_text()
            .ok()
            .and_then(|text| text.lines.into_iter().next())
            .unwrap_or_default();
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
                && usize::from(position.terminal_column)
                    <= self.display_id.width() + 1 + self.commit_title().width() =>
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
