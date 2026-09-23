use super::{ComposeScope, Control, ExploreComponent, controls::Button};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    widgets::{Paragraph, Widget},
};
use review_explore::ChangedLineCoverage;
use ui_theme::Palette;

#[derive(Clone)]
struct Item {
    area: Rect,
    button: Button,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum History {
    Previous,
    Next,
    Latest,
    Opening,
    Conclusion,
}

/// History controls stay visible while the question and its evidence scroll.
#[derive(Clone, Default)]
pub(super) struct Navigation {
    items: Vec<Item>,
    height: u16,
}

impl Navigation {
    fn new(area: Rect, labels: impl IntoIterator<Item = (String, Option<Control>)>) -> Self {
        let mut result = Self::default();
        let width = area.width.saturating_sub(2);
        for (row, buttons) in Button::wrap(width, labels)
            .into_iter()
            .enumerate()
            .take(usize::from(area.height))
        {
            if width == 0 {
                break;
            }
            let row = u16::try_from(row).expect("rows are bounded by the viewport");
            for button in buttons {
                let length = u16::try_from(button.text.len())
                    .unwrap_or(u16::MAX)
                    .min(width.saturating_sub(button.column));
                result.items.push(Item {
                    area: Rect::new(area.x + 1 + button.column, area.y + row, length, 1),
                    button,
                });
            }
            result.height = row + 1;
        }
        result
    }

    pub(super) fn height(&self) -> u16 {
        self.height
    }

    pub(super) fn control_at(&self, column: u16, row: u16) -> Option<Control> {
        self.items
            .iter()
            .find(|item| item.area.contains((column, row).into()))
            .and_then(|item| item.button.control)
    }

    pub(super) fn render(&self, buffer: &mut Buffer, palette: Palette) {
        for item in &self.items {
            Paragraph::new(item.button.text.as_str())
                .style(Style::default().fg(if item.button.control.is_some() {
                    palette.focus
                } else {
                    palette.text
                }))
                .render(item.area, buffer);
        }
    }
}

enum Page {
    Opening,
    Question(usize),
    Conclusion { request: String, number: usize },
}

impl Page {
    fn is_selected(&self, component: &ExploreComponent) -> bool {
        match self {
            Self::Opening => component.compose_scope == ComposeScope::Opening,
            Self::Question(index) => {
                component.compose_scope == ComposeScope::Question && *index == component.selected
            }
            Self::Conclusion { request, .. } => {
                component.compose_scope == ComposeScope::Conclusion
                    && component.general_context.as_ref() == Some(request)
            }
        }
    }

    fn label(&self, component: &ExploreComponent) -> String {
        match self {
            Self::Opening => "Opening".into(),
            Self::Question(index) => format!("Question {}/{}", index + 1, component.turns.len()),
            Self::Conclusion { number, .. } if component.conclusions.len() > 1 => {
                format!("Conclusion {number}/{}", component.conclusions.len())
            }
            Self::Conclusion { .. } => "Conclusion".into(),
        }
    }
}

/// Questions and conclusions retain the order in which the agent posted them.
struct HistoryPages {
    pages: Vec<Page>,
    current: usize,
    conclusion: Option<usize>,
}

impl HistoryPages {
    fn new(component: &ExploreComponent) -> Self {
        let mut pages = vec![Page::Opening];
        let mut question = 0;
        let mut conclusions = 0;
        let mut conclusion = None;
        for turn in component
            .exploration
            .iter()
            .flat_map(|exploration| &exploration.conversation)
        {
            if turn.update.next.is_some() {
                pages.push(Page::Question(question));
                question += 1;
            } else if turn.update.conclusion.is_some() {
                conclusions += 1;
                conclusion = Some(pages.len());
                pages.push(Page::Conclusion {
                    request: turn.update.request.clone(),
                    number: conclusions,
                });
            }
        }
        let current = pages
            .iter()
            .position(|page| page.is_selected(component))
            .unwrap_or(0);
        Self {
            pages,
            current,
            conclusion,
        }
    }

    fn destination(&self, target: History) -> usize {
        let last = self.pages.len() - 1;
        match target {
            History::Opening => 0,
            History::Previous => self.current.saturating_sub(1),
            History::Next => (self.current + 1).min(last),
            History::Latest => last,
            History::Conclusion => self.conclusion.unwrap_or(self.current),
        }
    }
}

impl ExploreComponent {
    pub(super) fn changed_line_percent(progress: ChangedLineCoverage) -> String {
        match progress.percent_tenths() {
            None => "—".into(),
            Some(0) if progress.explored > 0 => "<0.1%".into(),
            Some(tenths) if tenths % 10 == 0 => format!("{}%", tenths / 10),
            Some(tenths) => format!("{}.{:01}%", tenths / 10, tenths % 10),
        }
    }

    pub(super) fn visit_history(&mut self, target: History) {
        let history = HistoryPages::new(self);
        let destination = history.destination(target);
        if destination == history.current {
            return;
        }
        match &history.pages[destination] {
            Page::Opening => self.visit_opening(),
            Page::Question(index) => self.select(*index),
            Page::Conclusion { request, .. } => self.visit_conclusion_at(request.clone()),
        }
    }

    pub(super) fn navigation_bar(&self, area: Rect) -> Navigation {
        let history = HistoryPages::new(self);
        let last = history.pages.len() - 1;
        if last == 0 {
            let mut labels = self.coverage_control();
            labels.extend(self.execution_controls());
            labels.extend(self.saved_controls());
            return Navigation::new(area, labels);
        }
        let position = history.current;
        let mut labels = vec![(history.pages[position].label(self), None)];
        labels.extend(self.coverage_control());
        labels.extend(self.execution_controls());
        for (label, target, visible) in [
            ("Previous", History::Previous, position > 0),
            ("Next", History::Next, position < last),
            ("Latest", History::Latest, position < last),
            (
                "Conclusion",
                History::Conclusion,
                history.conclusion.is_some_and(|index| index != position),
            ),
            ("Opening", History::Opening, position > 0),
        ] {
            if visible {
                labels.push((format!("[{label}]"), Some(Control::History(target))));
            }
        }
        labels.extend(self.saved_controls());
        Navigation::new(area, labels)
    }

    fn coverage_control(&self) -> Vec<(String, Option<Control>)> {
        let Some(coverage) = &self.coverage else {
            return Vec::new();
        };
        let summary = coverage.summary(self.completion_policy.unwrap_or(self.jev_enabled));
        let label = if summary.complete {
            let lines = coverage.changed_line_coverage(None);
            if lines.total == 0 {
                "Coverage · No changed text lines".into()
            } else {
                format!(
                    "Coverage {} of changed lines",
                    Self::changed_line_percent(lines)
                )
            }
        } else {
            "Coverage incomplete".into()
        };
        vec![(
            format!(
                "[{label} {}]",
                if self.coverage_overview { "▴" } else { "▾" }
            ),
            Some(Control::Coverage),
        )]
    }

    fn execution_controls(&self) -> Vec<(String, Option<Control>)> {
        let timing = match self.compose_scope {
            ComposeScope::Question => self
                .question()
                .map(|question| self.execution_time(Some(question))),
            ComposeScope::Conclusion => Some(self.execution_time(None)),
            ComposeScope::Opening => None,
        };
        let mut labels = timing.map(|label| vec![(label, None)]).unwrap_or_default();
        if let Some(coverage) = &self.coverage
            && let Some(label) = Self::jev_progress(coverage)
        {
            labels.push((label, None));
        }
        labels
    }

    fn jev_progress(coverage: &review_explore::CoverageLedger) -> Option<String> {
        if !coverage.classification_started || coverage.jev_total_windows == 0 {
            return None;
        }
        let total = coverage.jev_total_windows;
        let done = coverage.classifications.len().min(total);
        let filled = done.saturating_mul(10) / total;
        let bar = format!("{}{}", "=".repeat(filled), "-".repeat(10 - filled));
        let label = if coverage.classification_finished {
            "Jev checked"
        } else {
            "Jev filtering"
        };
        Some(format!("{label} [{bar}] {done}/{total}"))
    }
}
