//! Checkpoint-bound, read-only preview of code not explored by answers.
use std::{cell::Cell, collections::BTreeMap};

use diff_component::DiffComponent;
use files_component::{FilePreviewList, PreviewFile};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    widgets::{Block, Borders, Paragraph, Widget},
};
use review_explore::{CoverageUnit, EvidenceRef, SourceSide};
use ui_controls::NavigationLink;
use ui_events::{
    EvidenceView, ExploreEvidence, ExploreEvidenceInput, PointerInput, PointerInputKind,
    ReviewPane, ReviewPaneFocusRequested,
};
use ui_panes::SplitPane;
use ui_shortcuts::Key;
use ui_theme::Palette;

use super::{ExploreComponent, flow::Window};

pub(super) struct ConclusionPreview {
    files: FilePreviewList,
    units: Vec<CoverageUnit>,
    header: Cell<Rect>,
    right: Cell<Rect>,
}

impl ExploreComponent {
    pub(super) fn open_conclusion_preview(&mut self) {
        let Some((request, unexplored)) = &self.conclusion_unexplored else {
            return;
        };
        if self.general_context.as_ref() != Some(request) {
            return;
        }
        let Some(exploration) = &self.exploration else {
            return;
        };
        let mut counts = BTreeMap::<usize, u64>::new();
        for unit in &unexplored.required {
            let value = match unit {
                CoverageUnit::Lines { first, end, .. } => u64::from(end - first),
                CoverageUnit::Item { .. } => 1,
            };
            *counts.entry(unit.file_index()).or_default() += value;
        }
        let files = counts
            .into_iter()
            .filter_map(|(file, required)| {
                exploration
                    .comparison
                    .files
                    .get(file)
                    .map(|changed| PreviewFile {
                        file,
                        path: changed.review_path().display(),
                        required,
                    })
            })
            .collect();
        let units = unexplored.required.clone();
        self.conclusion_preview = Some(ConclusionPreview {
            files: FilePreviewList::new(files),
            units,
            header: Cell::new(Rect::default()),
            right: Cell::new(Rect::default()),
        });
        self.open_selected_preview_file();
        self.events
            .publish(ReviewPaneFocusRequested(ReviewPane::Navigation));
    }

    fn open_selected_preview_file(&self) {
        let (Some(preview), Some(exploration)) = (&self.conclusion_preview, &self.exploration)
        else {
            return;
        };
        let Some(selected) = preview.files.selected() else {
            return;
        };
        let Some(file) = exploration.comparison.files.get(selected.file) else {
            return;
        };
        let evidence = preview
            .units
            .iter()
            .filter(|unit| unit.file_index() == selected.file)
            .map(|unit| {
                let (side, lines) = match unit {
                    CoverageUnit::Lines {
                        side, first, end, ..
                    } => (
                        *side,
                        Some(review_guide::GuideLineRange {
                            first_line: *first,
                            last_line: end - 1,
                        }),
                    ),
                    CoverageUnit::Item { .. } if file.new_path.is_some() => (SourceSide::New, None),
                    CoverageUnit::Item { .. } => (SourceSide::Old, None),
                };
                let path = match side {
                    SourceSide::Old => file.old_path.as_ref(),
                    SourceSide::New => file.new_path.as_ref(),
                }
                .unwrap_or_else(|| file.review_path())
                .clone();
                EvidenceRef {
                    location: review_explore::CodeLocation { path, side, lines },
                    notes: "Unexplored at conclusion".into(),
                }
            })
            .collect::<Vec<_>>();
        self.events.publish(ExploreEvidence {
            comparison: exploration.comparison.clone(),
            primary: evidence.len(),
            evidence,
            view: EvidenceView::Coverage,
            reveal: true,
            required_only: true,
        });
    }

    pub(super) fn preview_key(&mut self, key: Key) {
        if matches!(key, Key::Escape | Key::Char('b')) {
            self.conclusion_preview = None;
            self.events
                .publish(ReviewPaneFocusRequested(ReviewPane::Navigation));
            return;
        }
        if key == Key::Enter {
            self.events
                .publish(ReviewPaneFocusRequested(ReviewPane::Detail));
            return;
        }
        let changed = self
            .conclusion_preview
            .as_mut()
            .is_some_and(|view| view.files.move_key(key));
        if changed {
            self.open_selected_preview_file();
        }
    }

    pub(super) fn preview_pointer(&mut self, input: PointerInput) {
        let Some(position) = input.position else {
            return;
        };
        let (column, row) = (position.terminal_column, position.terminal_row);
        if self.preview_header_clicked(input.kind, column, row) {
            self.conclusion_preview = None;
            self.events
                .publish(ReviewPaneFocusRequested(ReviewPane::Navigation));
            return;
        }
        if self.preview_file_pointer(input.kind, column, row) {
            return;
        }
        self.preview_diff_pointer(input, column, row);
    }

    fn preview_header_clicked(&self, kind: PointerInputKind, column: u16, row: u16) -> bool {
        kind == PointerInputKind::Click
            && self.conclusion_preview.as_ref().is_some_and(|preview| {
                let header = preview.header.get();
                let width = NavigationLink::new("Back to conclusion").text().len();
                row == header.y && column >= header.x && usize::from(column - header.x) < width
            })
    }

    fn preview_file_pointer(&mut self, kind: PointerInputKind, column: u16, row: u16) -> bool {
        let Some(preview) = &mut self.conclusion_preview else {
            return false;
        };
        if !preview.files.contains(column, row) {
            return false;
        }
        match kind {
            PointerInputKind::Click => {
                if preview.files.select_at(column, row) {
                    self.open_selected_preview_file();
                }
                self.events
                    .publish(ReviewPaneFocusRequested(ReviewPane::Navigation));
            }
            PointerInputKind::Scroll(delta) => preview.files.scroll_by(delta),
            _ => {}
        }
        true
    }

    fn preview_diff_pointer(&self, mut input: PointerInput, column: u16, row: u16) {
        let Some(preview) = &self.conclusion_preview else {
            return;
        };
        let right = preview.right.get();
        if column >= right.x && column < right.right() && row >= right.y && row < right.bottom() {
            let window = Window::new(EvidenceView::Coverage, right, 0, right.height);
            if let Some(position) = &mut input.position {
                (position.component_column, position.component_row) =
                    window.viewport.local_position(column, row);
            }
            if input.kind == PointerInputKind::Click {
                self.events
                    .publish(ReviewPaneFocusRequested(ReviewPane::Detail));
            }
            self.events.publish(ExploreEvidenceInput {
                view: EvidenceView::Coverage,
                input,
            });
        }
    }

    pub(super) fn render_conclusion_preview(
        &self,
        area: Rect,
        buffer: &mut Buffer,
        palette: Palette,
        focused: bool,
        diff: &DiffComponent,
    ) {
        let Some(preview) = &self.conclusion_preview else {
            return;
        };
        let header = Rect::new(area.x, area.y, area.width, 2.min(area.height));
        preview.header.set(header);
        let checkpoint = self
            .exploration
            .as_ref()
            .map_or("", |pass| pass.comparison.checkpoint.checkpoint.as_str());
        let back = NavigationLink::new("Back to conclusion");
        let back_width = u16::try_from(back.text().len())
            .unwrap_or(u16::MAX)
            .min(header.width);
        back.render(
            Rect::new(header.x, header.y, back_width, 1),
            buffer,
            palette,
        );
        Paragraph::new(format!(" · Unexplored code at checkpoint {checkpoint}"))
            .style(Style::default().fg(palette.text))
            .render(
                Rect::new(
                    header.x.saturating_add(back_width),
                    header.y,
                    header.width.saturating_sub(back_width),
                    1,
                ),
                buffer,
            );
        Paragraph::new("↑/↓ files · Enter diff · b/Esc back from Files")
            .style(Style::default().fg(palette.text))
            .render(
                Rect::new(header.x, header.y.saturating_add(1), header.width, 1),
                buffer,
            );
        let body = Rect::new(
            area.x,
            area.y.saturating_add(header.height),
            area.width,
            area.height.saturating_sub(header.height),
        );
        SplitPane::new(body, (body.width / 3).max(22), 24).render(
            buffer,
            |left, buffer| preview.files.render(left, buffer, palette, focused),
            |right, buffer| {
                if right.width == 0 || right.height == 0 {
                    return;
                }
                let block = Block::default()
                    .borders(Borders::ALL)
                    .title(" Required checkpoint changes ")
                    .border_style(Style::default().fg(if focused {
                        palette.dim
                    } else {
                        palette.focus
                    }));
                let inner = block.inner(right);
                preview.right.set(inner);
                block.render(right, buffer);
                if preview.files.is_empty() {
                    Paragraph::new("No unexplored changed code").render(inner, buffer);
                } else if let Some(viewer) = diff.evidence_view(EvidenceView::Coverage) {
                    viewer.render_embedded(
                        Window::new(EvidenceView::Coverage, inner, 0, inner.height).viewport,
                        buffer,
                        palette,
                        !focused,
                    );
                } else {
                    Paragraph::new("Checkpoint diff is loading or unavailable")
                        .render(inner, buffer);
                }
            },
        );
    }
}
