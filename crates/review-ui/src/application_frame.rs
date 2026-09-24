//! Side-effect-free composition of mounted UI components.

use diff_component::DiffComponent;
use files_component::FilesComponent;
use guide_component::GuideComponent;
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
use ui_panes::SplitPane;
use ui_theme::Palette;

use crate::layout::{NavigationTabs, PaneLayout, location_selector_panes};

/// One renderable frame assembled from mounted components.
pub struct ApplicationFrame<'a> {
    pub(super) file_width: Option<u16>,
    pub(super) focus: ReviewPane,
    pub(super) palette: Palette,
    pub(super) files: &'a FilesComponent,
    pub(super) threads: &'a ThreadsComponent,
    pub(super) explore: &'a explore_component::ExploreComponent,
    pub(super) diff: &'a DiffComponent,
    pub(super) guide: &'a GuideComponent,
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

        let layout = PaneLayout::for_navigation(
            area.width,
            area.height,
            self.file_width,
            self.threads.mode(),
        );
        let header = Rect::new(area.x, area.y, area.width, 1);
        let body = Rect::new(area.x, area.y + 1, area.width, layout.body_height());
        let footer = Rect::new(
            area.x,
            area.bottom() - layout.footer_height,
            area.width,
            layout.footer_height,
        );
        self.status.render_header(header, buffer, self.palette);
        self.render_body(layout, body, buffer);
        self.diff.capture_reply_frame(buffer);
        self.status.render_footer(footer, buffer, self.palette);
        self.overlay.render_notifications(body, buffer);
        self.overlay.render(area, buffer);
        self.revision.render(area, buffer);
        self.diff.finish_reply_frame(buffer);
    }
}

impl ApplicationFrame<'_> {
    fn render_body(&self, layout: PaneLayout, body: Rect, buffer: &mut Buffer) {
        if self.threads.mode() == ReviewNavigation::Explore {
            let source = |area, buffer: &mut Buffer| {
                self.explore.render(
                    area,
                    buffer,
                    self.palette,
                    self.focus == ReviewPane::Navigation,
                    self.diff,
                );
                self.render_tabs(area, buffer);
            };
            if self.locations.is_active() {
                location_selector_panes(body, self.file_width).render(
                    buffer,
                    |left, buffer| self.locations.render(left, buffer, true),
                    source,
                );
            } else {
                source(body, buffer);
            }
            return;
        }
        if layout.is_wide() {
            SplitPane::new(body, layout.file_width, 0).render(
                buffer,
                |left, buffer| self.render_files(left, buffer),
                |right, buffer| self.render_diff(right, buffer),
            );
        } else {
            match self.focus {
                ReviewPane::Navigation => self.render_files(body, buffer),
                ReviewPane::Detail => self.render_diff(body, buffer),
            }
        }
    }

    fn render_files(&self, area: Rect, buffer: &mut Buffer) {
        if self.locations.is_active() {
            self.locations.render(area, buffer, true);
            return;
        }
        let mode = self.threads.mode();
        match mode {
            ReviewNavigation::Files => {
                self.files.render(
                    area,
                    buffer,
                    self.palette,
                    self.focus == ReviewPane::Navigation,
                );
            }
            ReviewNavigation::Explore => self.explore.render(
                area,
                buffer,
                self.palette,
                self.focus == ReviewPane::Navigation,
                self.diff,
            ),
            ReviewNavigation::Threads => {
                self.threads.render(
                    area,
                    buffer,
                    self.palette,
                    self.focus == ReviewPane::Navigation,
                );
            }
        }
        self.render_tabs(area, buffer);
    }

    fn render_tabs(&self, area: Rect, buffer: &mut Buffer) {
        let mode = self.threads.mode();
        let active = Style::default()
            .fg(self.palette.focus)
            .add_modifier(Modifier::BOLD);
        let inactive = Style::default().fg(self.palette.dim);
        let tabs = Line::from(vec![
            Span::styled(
                NavigationTabs::FILES,
                if mode == ReviewNavigation::Files {
                    active
                } else {
                    inactive
                },
            ),
            Span::raw(NavigationTabs::SEPARATOR),
            Span::styled(
                NavigationTabs::THREADS,
                if mode == ReviewNavigation::Threads {
                    active
                } else {
                    inactive
                },
            ),
            Span::styled(
                if self.threads.has_unread_replies() {
                    NavigationTabs::UNREAD
                } else {
                    ""
                },
                Style::default().fg(self.palette.deletion),
            ),
            Span::raw(NavigationTabs::SEPARATOR),
            Span::styled(
                NavigationTabs::EXPLORE,
                if mode == ReviewNavigation::Explore {
                    active
                } else {
                    inactive
                },
            ),
        ]);
        let width = u16::try_from(tabs.width())
            .unwrap_or(u16::MAX)
            .min(area.width.saturating_sub(2));
        Paragraph::new(tabs).render(Rect::new(area.x + 1, area.y, width, 1), buffer);
    }

    fn render_diff(&self, area: Rect, buffer: &mut Buffer) {
        let viewport = self.diff.displayed_guide_viewport();
        let guide_layout = viewport
            .as_ref()
            .map(|viewport| self.guide.layout(viewport, self.palette.guide));
        let overlay = self.diff.render(
            area,
            buffer,
            self.palette,
            self.focus == ReviewPane::Detail,
            guide_layout,
        );
        overlay.render(buffer);
    }
}
