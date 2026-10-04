//! Side-effect-free composition of mounted UI components.

use diff_component::DiffComponent;
use files_component::FilesComponent;
use locations_component::LocationsComponent;
use overlay_component::OverlayComponent;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use revision_component::RevisionComponent;
use status_component::StatusComponent;
use threads_component::ThreadsComponent;
use ui_events::{ReviewNavigation, ReviewPane};
use ui_theme::Palette;

use crate::layout::{Body, NavigationTabs, ScreenLayout};

/// One renderable frame assembled from mounted components.
pub struct ApplicationFrame<'a> {
    pub(super) layout: ScreenLayout,
    pub(super) mode: ReviewNavigation,
    pub(super) focus: ReviewPane,
    pub(super) palette: Palette,
    pub(super) files: &'a FilesComponent,
    pub(super) threads: &'a ThreadsComponent,
    pub(super) explore: &'a explore_component::ExploreComponent,
    pub(super) diff: &'a DiffComponent,
    pub(super) locations: &'a LocationsComponent,
    pub(super) status: &'a StatusComponent,
    pub(super) overlay: &'a OverlayComponent,
    pub(super) revision: &'a RevisionComponent,
}

impl Widget for ApplicationFrame<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        self.diff.begin_reply_frame();
        if self.status.render_terminal_too_small(area, buffer) {
            return;
        }

        let layout = self.layout.fitted_to(area);
        let body = layout.body_area();
        self.status
            .render_header(layout.header(), buffer, self.palette);
        self.render_body(layout.body(), buffer);
        let hints = if self.mode == ReviewNavigation::Explore {
            self.explore.footer_hints()
        } else {
            Vec::new()
        };
        self.status
            .render_footer(layout.footer(), buffer, self.palette, &hints);
        self.overlay.render_notifications(body, buffer);
        self.overlay.render(area, buffer);
        self.revision.render(area, buffer);
        self.diff.finish_reply_frame(buffer);
    }
}

impl ApplicationFrame<'_> {
    fn render_body(&self, body: Body, buffer: &mut Buffer) {
        match body {
            Body::Explore { locations, source } => {
                if let Some(split) = locations {
                    self.locations.render(split.left, buffer, true);
                }
                self.render_navigation(source, buffer);
            }
            Body::Split(split) => {
                self.render_navigation_pane(split.left, buffer);
                self.render_diff(split.right, buffer);
            }
            Body::Single {
                pane: ReviewPane::Navigation,
                area,
            } => self.render_navigation_pane(area, buffer),
            Body::Single {
                pane: ReviewPane::Detail,
                area,
            } => self.render_diff(area, buffer),
        }
    }

    /// The navigation pane, which the location selector replaces while open.
    fn render_navigation_pane(&self, area: Rect, buffer: &mut Buffer) {
        if self.locations.is_active() {
            self.locations.render(area, buffer, true);
        } else {
            self.render_navigation(area, buffer);
        }
    }

    fn render_navigation(&self, area: Rect, buffer: &mut Buffer) {
        let focused = self.focus == ReviewPane::Navigation;
        match self.mode {
            ReviewNavigation::Files => self.files.render(area, buffer, self.palette, focused),
            ReviewNavigation::Threads => self.threads.render(area, buffer, self.palette, focused),
            ReviewNavigation::Explore => {
                self.explore
                    .render(area, buffer, self.palette, focused, self.diff);
            }
        }
        self.render_tabs(area, buffer);
    }

    /// The tabs over the navigation pane's top border: the open one is a
    /// filled pill, and each underlines the letter that opens it.
    fn render_tabs(&self, area: Rect, buffer: &mut Buffer) {
        let palette = self.palette;
        let tabs = NavigationTabs::new(self.threads.has_unread_replies());
        let mut spans = Vec::new();
        for tab in tabs.tabs() {
            if !spans.is_empty() {
                spans.push(Span::raw(NavigationTabs::GAP));
            }
            let style = if tab.mode == self.mode {
                Style::default()
                    .bg(palette.focus)
                    .fg(palette.background)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(palette.dim)
            };
            let (key, rest) = tab.key_and_rest();
            spans.extend([
                Span::styled(NavigationTabs::PADDING, style),
                Span::styled(key, style.add_modifier(Modifier::UNDERLINED)),
                Span::styled(rest, style),
                Span::styled(NavigationTabs::PADDING, style),
            ]);
            if tab.unread {
                spans.push(Span::styled(
                    NavigationTabs::UNREAD,
                    Style::default().fg(palette.deletion),
                ));
            }
        }
        let tabs = Line::from(spans);
        let width = u16::try_from(tabs.width())
            .unwrap_or(u16::MAX)
            .min(area.width.saturating_sub(2));
        Paragraph::new(tabs).render(Rect::new(area.x + 1, area.y, width, 1), buffer);
    }

    fn render_diff(&self, area: Rect, buffer: &mut Buffer) {
        self.diff
            .render(area, buffer, self.palette, self.focus == ReviewPane::Detail);
    }
}
