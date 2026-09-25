//! Decision evidence and optional supporting sources share native viewers.
use super::{
    ExploreComponent,
    flow::{Content, ConversationLayout, VisibleContent, Window, evidence_panes},
};
use diff_component::DiffComponent;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
    widgets::{Paragraph, Widget},
};
use review_explore::SourceSide;
use std::collections::HashSet;
use ui_events::EvidenceView;
use ui_panes::{FileList, FileTree, FileTreeRow, SelectionPane, pad_to_width};
use ui_shortcuts::NavigationShortcut;
use ui_theme::Palette;

#[derive(Clone)]
struct EvidenceEntry {
    view: EvidenceView,
    path: String,
    primary: bool,
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
    primary: usize,
    pub(super) width: Option<u16>,
}

impl EvidenceList {
    fn new(
        exploration: &review_explore::Exploration,
        turn: usize,
        selected: usize,
        width: Option<u16>,
    ) -> Self {
        let primary = exploration.questions[turn].evidence.len();
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
                    view: EvidenceView::Question {
                        turn,
                        reference: index,
                    },
                    path,
                    primary: index < primary,
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
        let view = EvidenceView::Question {
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
            primary,
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
        let supporting = self.entries.len().saturating_sub(self.primary);
        FileList {
            tree: &self.tree,
            selected: self.selected,
            scroll: self.first_visible(full_height) + usize::from(skipped),
            page_rows: usize::from(area.height.saturating_sub(2)),
        }
        .render(
            area,
            buffer,
            palette,
            focused,
            &format!("Evidence {} · Supporting {supporting}", self.primary),
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
                let mut style = Style::default().fg(if entry.primary {
                    palette.text
                } else {
                    palette.dim
                });
                if entry.primary {
                    style = style.add_modifier(Modifier::BOLD);
                }
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
        let index =
            SelectionPane::new(area, self.first_visible(full_height) + usize::from(skipped))
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

    pub(super) fn navigate(&self, input: NavigationShortcut, height: u16) -> Option<EvidenceView> {
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
        let view = EvidenceView::Question {
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
        if let Some(viewer) = diff.evidence_view(view) {
            if let Some(limitation) = viewer.evidence_limitation() {
                list.source_available = false;
                layout.text(limitation, palette.warning, None);
                layout.push(Content::EvidenceSplit(list), 8);
            } else {
                let maximum = layout.area.height.saturating_sub(1).max(3);
                let height = self
                    .heights
                    .get(&view)
                    .copied()
                    .unwrap_or_else(|| {
                        viewer.fitted_evidence_height(
                            super::flow::evidence_panes(layout.area, self.evidence_width)
                                .right
                                .width,
                            (layout.area.height / 2).max(3),
                        )
                    })
                    .max(
                        u16::try_from(evidence.len().saturating_add(2))
                            .unwrap_or(u16::MAX)
                            .min((layout.area.height / 2).max(3)),
                    )
                    .clamp(3, maximum);
                layout.push(Content::EvidenceSplit(list), height);
            }
        } else {
            list.source_available = false;
            layout.push(Content::EvidenceSplit(list), 8);
        }
        layout.evidence = Some(code_start..layout.height);
    }
}
