//! Every citation of a question, its assessments and the agent's reply shares
//! one evidence list and its native viewers.
use super::{
    ExploreComponent,
    flow::{Content, ConversationLayout, VisibleContent, Window, evidence_panes},
};
use diff_component::{ClippedViewport, DiffComponent};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::Line,
    widgets::{Paragraph, Widget},
};
use review_explore::SourceSide;
use std::collections::HashSet;
use ui_events::EvidenceView;
use ui_panes::{FileList, FileTree, FileTreeRow, SelectionPane, pad_to_width};
use ui_shortcuts::MovementShortcut;
use ui_theme::Palette;

#[derive(Clone)]
struct EvidenceEntry {
    view: EvidenceView,
    path: String,
    side: SourceSide,
    first_line: Option<u32>,
}

/// The question's sources remain selectable beside the current source window.
#[derive(Clone)]
pub(super) struct EvidenceList {
    pub(super) view: EvidenceView,
    pub(super) source_available: bool,
    entries: Vec<EvidenceEntry>,
    tree: FileTree,
    selected: usize,
    pub(super) width: Option<u16>,
}

impl EvidenceList {
    fn new(
        exploration: &review_explore::Exploration,
        turn: usize,
        selected: usize,
        width: Option<u16>,
    ) -> Self {
        let entries = exploration
            .evidence(turn)
            .iter()
            .enumerate()
            .map(|(index, evidence)| {
                let path = exploration
                    .comparison
                    .source(&evidence.location)
                    .map_or_else(
                        || "Unavailable".to_owned(),
                        |source| source.display_path.clone(),
                    );
                EvidenceEntry {
                    view: EvidenceView {
                        turn,
                        reference: index,
                    },
                    path,
                    side: evidence.location.side,
                    first_line: evidence
                        .location
                        .lines
                        .as_ref()
                        .map(|lines| lines.first_line),
                }
            })
            .collect::<Vec<_>>();
        let mut entries = entries;
        entries.sort_by(|left, right| left.path.cmp(&right.path));
        let tree = FileTree::new(
            entries
                .iter()
                .map(|entry| (entry.path.clone(), entry.path.clone())),
            &HashSet::new(),
        );
        let view = EvidenceView {
            turn,
            reference: selected,
        };
        let selected = entries
            .iter()
            .position(|entry| entry.view == view)
            .unwrap_or(0);
        Self {
            view,
            source_available: true,
            entries,
            tree,
            selected,
            width,
        }
    }

    fn first_visible(&self, full_height: u16) -> usize {
        let rows = usize::from(full_height.saturating_sub(2).max(1));
        let selected_row = self.tree.row_for_file(self.selected).unwrap_or(0);
        selected_row
            .saturating_sub(rows / 2)
            .min(self.tree.rows.len().saturating_sub(rows))
    }

    pub(super) fn render(
        &self,
        area: Rect,
        buffer: &mut Buffer,
        palette: Palette,
        skipped: u16,
        full_height: u16,
        focused: bool,
    ) {
        let viewport = ClippedViewport::new(area, skipped, full_height).with_pinned_header();
        let mut pane = Buffer::empty(viewport.area());
        FileList {
            tree: &self.tree,
            selected: self.selected,
            scroll: self.first_visible(full_height),
            page_rows: usize::from(full_height.saturating_sub(2)),
        }
        .render(
            viewport.area(),
            &mut pane,
            palette,
            focused,
            &format!("Evidence {}", self.entries.len()),
            |depth, name, file, width| {
                let entry = &self.entries[file];
                let line = entry
                    .first_line
                    .map_or_else(String::new, |line| format!(":{line}"));
                let side = if entry.side == SourceSide::Old {
                    " (base)"
                } else {
                    ""
                };
                let label = format!("{}{name}{line}{side}", "  ".repeat(depth));
                let mut style = Style::default().fg(palette.text);
                if file == self.selected {
                    style = style.bg(palette.cursor);
                }
                Line::styled(
                    if file == self.selected {
                        pad_to_width(&label, width)
                    } else {
                        ui_panes::shorten(&label, width)
                    },
                    style,
                )
            },
        );
        viewport.draw(&pane, buffer);
    }

    pub(super) fn render_split(
        &self,
        visible: VisibleContent<'_>,
        buffer: &mut Buffer,
        diff: &DiffComponent,
        palette: Palette,
        source_focused: bool,
        list_focused: bool,
    ) {
        let panes = evidence_panes(visible.area, self.width);
        panes.render(
            buffer,
            |left, buffer| {
                self.render(
                    left,
                    buffer,
                    palette,
                    visible.skipped,
                    visible.item.height,
                    list_focused,
                );
            },
            |right, buffer| {
                if let Some(viewer) = diff.evidence_view(self.view) {
                    if let Some(limitation) = viewer.evidence_limitation() {
                        Paragraph::new(limitation).render(right, buffer);
                    } else {
                        viewer.render_embedded(
                            Window::new(self.view, right, visible.skipped, visible.item.height)
                                .viewport,
                            buffer,
                            palette,
                            source_focused,
                        );
                    }
                } else {
                    Paragraph::new("Evidence is loading or unavailable").render(right, buffer);
                }
            },
        );
        panes.render_divider(buffer, Style::default().fg(palette.dim));
    }

    pub(super) fn control_at(
        &self,
        area: Rect,
        column: u16,
        row: u16,
        skipped: u16,
        full_height: u16,
    ) -> Option<EvidenceView> {
        let viewport = ClippedViewport::new(area, skipped, full_height).with_pinned_header();
        let (column, row) = viewport.local_position(column, row);
        let index = SelectionPane::new(viewport.area(), self.first_visible(full_height))
            .index_at(column, row)?;
        match self.tree.rows.get(index)? {
            FileTreeRow::File { file, .. } => self.entries.get(*file).map(|entry| entry.view),
            FileTreeRow::Directory { .. } => None,
        }
    }

    pub(super) fn scroll_at(
        &self,
        area: Rect,
        column: u16,
        row: u16,
        delta: isize,
    ) -> Option<EvidenceView> {
        if !area.contains((column, row).into()) {
            return None;
        }
        let next = self
            .selected
            .saturating_add_signed(delta)
            .min(self.entries.len().saturating_sub(1));
        self.entries.get(next).map(|entry| entry.view)
    }

    pub(super) fn navigate(&self, input: MovementShortcut, height: u16) -> Option<EvidenceView> {
        let next = FileList {
            tree: &self.tree,
            selected: self.selected,
            scroll: self.first_visible(height),
            page_rows: usize::from(height.saturating_sub(2).max(1)),
        }
        .navigate(input);
        self.entries.get(next).map(|entry| entry.view)
    }
}

impl ExploreComponent {
    pub(super) fn evidence_block(
        &self,
        index: usize,
        layout: &mut ConversationLayout,
        diff: &DiffComponent,
        palette: Palette,
    ) {
        let exploration = self.exploration.as_ref().expect("question exploration");
        let reference_index = self.turns[index].reference;
        let view = EvidenceView {
            turn: index,
            reference: reference_index,
        };
        let evidence = exploration.evidence(index);
        let Some(reference) = evidence.get(reference_index) else {
            return;
        };
        layout.section("Notes", &reference.notes, palette);
        if !reference.notes.trim().is_empty() {
            layout.gap();
        }
        let code_start = layout.height;
        let mut list = EvidenceList::new(exploration, index, reference_index, self.evidence_width);
        let height = self.evidence_height(view, layout);
        if let Some(viewer) = diff.evidence_view(view) {
            if let Some(limitation) = viewer.evidence_limitation() {
                list.source_available = false;
                layout.text(limitation, palette.warning, None);
            }
        } else {
            list.source_available = false;
        }
        layout.push(Content::EvidenceSplit(list), height);
        layout.evidence = Some(code_start..layout.height);
    }

    fn evidence_height(&self, view: EvidenceView, layout: &ConversationLayout) -> u16 {
        self.heights
            .get(&view)
            .copied()
            .unwrap_or((layout.area.height.saturating_sub(4) / 2).max(3))
            .clamp(3, layout.area.height.saturating_sub(1).max(3))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrolling_clips_the_file_list_without_moving_its_bottom_border() {
        let entries = (0..10)
            .map(|reference| EvidenceEntry {
                view: EvidenceView { turn: 0, reference },
                path: format!("file{reference}.rs"),
                side: SourceSide::New,
                first_line: None,
            })
            .collect::<Vec<_>>();
        let tree = FileTree::new(
            entries
                .iter()
                .map(|entry| (entry.path.clone(), entry.path.clone())),
            &HashSet::new(),
        );
        let list = EvidenceList {
            view: EvidenceView {
                turn: 0,
                reference: 0,
            },
            source_available: true,
            entries,
            tree,
            selected: 0,
            width: None,
        };
        let area = Rect::new(0, 8, 20, 4);
        let mut buffer = Buffer::empty(Rect::new(0, 0, 20, 12));
        list.render(
            area,
            &mut buffer,
            ui_theme::Theme::default().palette,
            5,
            12,
            false,
        );

        assert_eq!(buffer[(0, 11)].symbol(), "│");
        assert_eq!(buffer[(19, 11)].symbol(), "│");
        assert_eq!(
            list.control_at(area, 2, 9, 5, 12),
            Some(EvidenceView {
                turn: 0,
                reference: 5,
            })
        );
    }
}
