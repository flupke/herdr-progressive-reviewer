//! Revision selector state, input handling, layout, and rendering.

use ansi_to_tui::IntoText;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget};
use review_repository::repository::{
    ChangeId, RevisionCandidate, RevisionDirection, RevisionHistoryLine,
};
use ui_shortcuts::Key;
use ui_theme::Palette;

pub(super) struct RevisionHistorySelection {
    lines: Vec<RevisionHistoryLine>,
    selected_line: usize,
    filter: Option<RevisionHistoryFilter>,
}

#[derive(Default)]
struct RevisionHistoryFilter {
    query: String,
}

pub(super) enum RevisionHistoryInputResult {
    None,
    Close,
    Edit(ChangeId),
}

pub(super) struct SelectableRevision {
    direction: RevisionDirection,
    candidate: RevisionCandidate,
}

impl RevisionHistoryFilter {
    fn match_priority(&self, line: &RevisionHistoryLine) -> Option<u8> {
        if line
            .short_change_id
            .as_ref()
            .is_some_and(|short_change_id| {
                starts_with_ignore_ascii_case(short_change_id, &self.query)
            })
        {
            return Some(0);
        }
        fuzzy_matches(&line.plain_text, &self.query).then_some(1)
    }
}

fn starts_with_ignore_ascii_case(text: &str, prefix: &str) -> bool {
    text.get(..prefix.len())
        .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
}

fn fuzzy_matches(text: &str, query: &str) -> bool {
    let mut query = query.chars();
    let Some(mut expected) = query.next() else {
        return true;
    };
    for character in text.chars() {
        if character.eq_ignore_ascii_case(&expected) {
            let Some(next) = query.next() else {
                return true;
            };
            expected = next;
        }
    }
    false
}

impl RevisionHistorySelection {
    pub(super) fn new(lines: Vec<RevisionHistoryLine>) -> Self {
        let selected_line = lines
            .iter()
            .position(|line| line.is_current)
            .or_else(|| lines.iter().position(|line| line.change_id.is_some()))
            .unwrap_or_default();
        Self {
            lines,
            selected_line,
            filter: None,
        }
    }

    pub(super) fn row_count(&self) -> usize {
        self.lines.len()
    }

    pub(super) fn handle_key(&mut self, key: Key) -> RevisionHistoryInputResult {
        if self.filter.is_some() {
            self.handle_filter_key(key)
        } else {
            self.handle_navigation_key(key)
        }
    }

    fn match_priority(&self, line: &RevisionHistoryLine) -> Option<u8> {
        if line.is_immutable || line.change_id.is_none() {
            return None;
        }
        self.filter
            .as_ref()
            .map_or(Some(0), |filter| filter.match_priority(line))
    }

    fn best_matching_line(&self) -> Option<usize> {
        self.lines
            .iter()
            .enumerate()
            .filter_map(|(index, line)| self.match_priority(line).map(|priority| (priority, index)))
            .min()
            .map(|(_, index)| index)
    }

    fn move_next(&mut self) {
        if let Some(next) = self
            .lines
            .iter()
            .enumerate()
            .skip(self.selected_line.saturating_add(1))
            .find_map(|(index, line)| self.is_selectable(line).then_some(index))
        {
            self.selected_line = next;
        }
    }

    fn move_previous(&mut self) {
        if let Some(previous) = self.lines[..self.selected_line.min(self.lines.len())]
            .iter()
            .rposition(|line| self.is_selectable(line))
        {
            self.selected_line = previous;
        }
    }

    fn start_filter(&mut self) {
        self.filter = Some(RevisionHistoryFilter::default());
    }

    fn update_filter(&mut self, character: Option<char>) {
        let Some(filter) = &mut self.filter else {
            return;
        };
        if let Some(character) = character {
            filter.query.push(character.to_ascii_lowercase());
        } else {
            filter.query.pop();
        }
        if filter.query.is_empty() {
            return;
        }
        if let Some(matching_line) = self.best_matching_line() {
            self.selected_line = matching_line;
        }
    }

    fn stop_filter(&mut self) {
        self.filter = None;
    }

    fn query(&self) -> Option<&str> {
        self.filter.as_ref().map(|filter| filter.query.as_str())
    }

    fn is_selectable(&self, line: &RevisionHistoryLine) -> bool {
        self.match_priority(line).is_some()
    }

    fn selected_change_id(&self) -> Option<ChangeId> {
        self.lines
            .get(self.selected_line)
            .filter(|line| self.is_selectable(line))
            .and_then(|line| line.change_id.clone())
    }

    fn handle_navigation_key(&mut self, key: Key) -> RevisionHistoryInputResult {
        match key {
            Key::Char('/') => self.start_filter(),
            Key::Char('j') | Key::Down => self.move_next(),
            Key::Char('k') | Key::Up => self.move_previous(),
            Key::Escape => return RevisionHistoryInputResult::Close,
            Key::Enter => return self.edit_result(),
            _ => {}
        }
        RevisionHistoryInputResult::None
    }

    fn handle_filter_key(&mut self, key: Key) -> RevisionHistoryInputResult {
        match key {
            Key::Down => self.move_next(),
            Key::Up => self.move_previous(),
            Key::Backspace => self.update_filter(None),
            Key::Char(character) if character.is_ascii_alphanumeric() => {
                self.update_filter(Some(character));
            }
            Key::Escape => self.stop_filter(),
            Key::Enter => return self.edit_result(),
            _ => {}
        }
        RevisionHistoryInputResult::None
    }

    fn edit_result(&self) -> RevisionHistoryInputResult {
        self.selected_change_id().map_or(
            RevisionHistoryInputResult::None,
            RevisionHistoryInputResult::Edit,
        )
    }
}

impl SelectableRevision {
    pub(super) const fn new(direction: RevisionDirection, candidate: RevisionCandidate) -> Self {
        Self {
            direction,
            candidate,
        }
    }

    pub(super) fn candidate(&self) -> &RevisionCandidate {
        &self.candidate
    }
}

struct SelectorPopup {
    area: Rect,
    inner: Rect,
}

impl SelectorPopup {
    fn new(terminal_area: Rect, row_count: usize) -> Self {
        let width = terminal_area.width.saturating_mul(4) / 5;
        let height = u16::try_from(row_count.max(1))
            .unwrap_or(u16::MAX)
            .saturating_add(2)
            .min(terminal_area.height);
        let area = Rect::new(
            terminal_area.x + terminal_area.width.saturating_sub(width) / 2,
            terminal_area.y + terminal_area.height.saturating_sub(height) / 2,
            width,
            height,
        );
        Self {
            area,
            inner: Block::default().borders(Borders::ALL).inner(area),
        }
    }

    fn render(
        &self,
        buffer: &mut Buffer,
        title: impl Into<ratatui::text::Line<'static>>,
        palette: &Palette,
        lines: Vec<Line<'_>>,
        viewport_line: usize,
        highlighted_line: Option<usize>,
    ) {
        Clear.render(self.area, buffer);
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(palette.focus))
            .render(self.area, buffer);
        let visible_rows = usize::from(self.inner.height).max(1);
        let scroll = viewport_line.saturating_add(1).saturating_sub(visible_rows);
        Paragraph::new(lines)
            .scroll((u16::try_from(scroll).unwrap_or(u16::MAX), 0))
            .render(self.inner, buffer);
        if let Some(highlighted_line) = highlighted_line {
            let visible_line = highlighted_line.saturating_sub(scroll);
            if visible_line < visible_rows {
                let highlight_area = Rect::new(
                    self.inner.x,
                    self.inner.y + u16::try_from(visible_line).unwrap_or(self.inner.height),
                    self.inner.width,
                    1,
                );
                buffer.set_style(highlight_area, Style::default().bg(palette.selection));
            }
        }
    }
}

pub(super) fn selector_area(terminal_area: Rect, row_count: usize) -> Rect {
    SelectorPopup::new(terminal_area, row_count).area
}

pub(super) fn render_loading(area: Rect, buffer: &mut Buffer, palette: &Palette) {
    SelectorPopup::new(area, 1).render(
        buffer,
        "Select revision",
        palette,
        vec![Line::raw("Loading revision history…")],
        0,
        None,
    );
}

pub(super) fn render_candidates(
    area: Rect,
    buffer: &mut Buffer,
    palette: &Palette,
    candidates: &[SelectableRevision],
    selected: usize,
) {
    let lines = candidates
        .iter()
        .map(|candidate| {
            let description = if candidate.candidate.description.is_empty() {
                "(no description set)"
            } else {
                &candidate.candidate.description
            };
            let relation = match candidate.direction {
                RevisionDirection::Parents => "parent",
                RevisionDirection::Children => "child ",
            };
            Line::raw(format!(
                "{relation}  {}  {description}",
                candidate.candidate.short_change_id
            ))
        })
        .collect::<Vec<_>>();
    let lines = if lines.is_empty() {
        vec![Line::from("No parent or child revisions")]
    } else {
        lines
    };
    SelectorPopup::new(area, candidates.len()).render(
        buffer,
        "Select revision",
        palette,
        lines,
        selected,
        Some(selected),
    );
}

pub(super) fn render_history(
    area: Rect,
    buffer: &mut Buffer,
    palette: &Palette,
    selection: &RevisionHistorySelection,
) {
    let title = match selection.query() {
        None => "Select revision".to_owned(),
        Some(query) => format!("Select revision: /{query}"),
    };
    let lines = selection
        .lines
        .iter()
        .map(|history_line| {
            history_line
                .text
                .as_bytes()
                .into_text()
                .ok()
                .and_then(|text| text.lines.into_iter().next())
                .unwrap_or_else(|| Line::raw(history_line.plain_text.clone()))
        })
        .collect::<Vec<_>>();
    SelectorPopup::new(area, selection.lines.len()).render(
        buffer,
        title,
        palette,
        lines,
        selection.selected_line,
        selection
            .lines
            .get(selection.selected_line)
            .filter(|line| selection.is_selectable(line))
            .map(|_| selection.selected_line),
    );
}
