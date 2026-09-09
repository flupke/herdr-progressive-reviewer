//! Side-effect-free composition of mounted UI components.

use diff_component::DiffComponent;
use files_component::FilesComponent;
use guide_component::GuideComponent;
use locations_component::LocationsComponent;
use overlay_component::OverlayComponent;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;
use revision_component::RevisionComponent;
use status_component::StatusComponent;
use ui_theme::Palette;

use crate::layout::{Focus, PaneLayout};

/// One renderable frame assembled from mounted components.
pub struct ApplicationFrame<'a> {
    pub(super) file_width: Option<u16>,
    pub(super) focus: Focus,
    pub(super) palette: Palette,
    pub(super) files: &'a FilesComponent,
    pub(super) diff: &'a DiffComponent,
    pub(super) guide: &'a GuideComponent,
    pub(super) locations: &'a LocationsComponent,
    pub(super) status: &'a StatusComponent,
    pub(super) overlay: &'a OverlayComponent,
    pub(super) revision: &'a RevisionComponent,
}

impl Widget for ApplicationFrame<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        if self.status.render_terminal_too_small(area, buffer) {
            return;
        }

        let layout = PaneLayout::new(area.width, area.height, self.file_width);
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
        self.status.render_footer(footer, buffer, self.palette);
        self.overlay.render_notifications(body, buffer);
        self.overlay.render(area, buffer);
        self.revision.render(area, buffer);
    }
}

impl ApplicationFrame<'_> {
    fn render_body(&self, layout: PaneLayout, body: Rect, buffer: &mut Buffer) {
        if layout.is_wide() {
            let file_width = layout.file_width;
            self.render_files(Rect::new(body.x, body.y, file_width, body.height), buffer);
            self.render_diff(
                Rect::new(
                    body.x + file_width,
                    body.y,
                    body.width - file_width,
                    body.height,
                ),
                buffer,
            );
        } else {
            match self.focus {
                Focus::Files => self.render_files(body, buffer),
                Focus::Diff => self.render_diff(body, buffer),
            }
        }
    }

    fn render_files(&self, area: Rect, buffer: &mut Buffer) {
        if self.locations.is_active() {
            self.locations.render(area, buffer, true);
            return;
        }
        self.files
            .render(area, buffer, self.palette, self.focus == Focus::Files);
    }

    fn render_diff(&self, area: Rect, buffer: &mut Buffer) {
        let viewport = self.diff.displayed_guide_viewport();
        let guide_layout = viewport
            .as_ref()
            .map(|viewport| self.guide.layout(viewport, self.palette.guide));
        let guide_overlay = self.diff.render(
            area,
            buffer,
            self.palette,
            self.focus == Focus::Diff,
            guide_layout,
        );
        self.guide.render(&guide_overlay, buffer);
    }
}
