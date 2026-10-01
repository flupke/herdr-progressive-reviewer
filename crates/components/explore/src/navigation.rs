use super::{
    ComposeScope, Control, ExploreComponent,
    controls::{Button, ControlVisual},
};
use ratatui::{buffer::Buffer, layout::Rect};
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
        for (row, buttons) in Button::wrap(
            width,
            labels
                .into_iter()
                .map(|(label, control)| (ControlVisual::Text(label), control)),
        )
        .into_iter()
        .enumerate()
        .take(usize::from(area.height))
        {
            if width == 0 {
                break;
            }
            let row = u16::try_from(row).expect("rows are bounded by the viewport");
            for button in buttons {
                let length = button.width().min(width.saturating_sub(button.column));
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
            item.button.render(item.area, buffer, palette);
        }
    }
}

enum Page {
    Question(usize),
    Conclusion { request: String, number: usize },
}

impl Page {
    fn is_selected(&self, component: &ExploreComponent) -> bool {
        match self {
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
        let mut pages = Vec::new();
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

    fn destination(&self, target: History) -> Option<usize> {
        let last = self.pages.len().checked_sub(1)?;
        match target {
            History::Previous => Some(self.current.saturating_sub(1)),
            History::Next => Some((self.current + 1).min(last)),
            History::Latest => Some(last),
            History::Conclusion => self.conclusion,
        }
    }
}

impl ExploreComponent {
    pub(super) fn visit_history(&mut self, target: History) {
        let history = HistoryPages::new(self);
        let Some(destination) = history.destination(target) else {
            return;
        };
        if destination == history.current {
            return;
        }
        match &history.pages[destination] {
            Page::Question(index) => self.select(*index),
            Page::Conclusion { request, .. } => self.visit_conclusion_at(request.clone()),
        }
    }

    pub(super) fn navigation_bar(&self, area: Rect) -> Navigation {
        let history = HistoryPages::new(self);
        let Some(last) = history.pages.len().checked_sub(1) else {
            return Navigation::new(area, self.execution_controls());
        };
        let position = history.current;
        let mut labels = vec![(history.pages[position].label(self), None)];
        labels.extend(self.execution_controls());
        for (label, target, visible) in [
            ("Previous", History::Previous, position > 0),
            (
                "Next",
                History::Next,
                position < last && history.conclusion != Some(position + 1),
            ),
            (
                "Latest",
                History::Latest,
                position.saturating_add(1) < last && history.conclusion != Some(last),
            ),
            (
                "Conclusion",
                History::Conclusion,
                history
                    .conclusion
                    .is_some_and(|index| index != position && index != position.saturating_sub(1)),
            ),
        ] {
            if visible {
                labels.push((format!("[{label}]"), Some(Control::History(target))));
            }
        }
        Navigation::new(area, labels)
    }

    fn execution_controls(&self) -> Vec<(String, Option<Control>)> {
        let timing = match self.compose_scope {
            ComposeScope::Question => self
                .question()
                .map(|question| self.execution_time(Some(question))),
            ComposeScope::Conclusion => Some(self.execution_time(None)),
        };
        timing.map(|label| vec![(label, None)]).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_controls_use_displayed_width_for_unicode_clicks() {
        let navigation = Navigation::new(
            Rect::new(0, 0, 40, 1),
            [
                ("Map view ▾".into(), Some(Control::Map)),
                ("Next".into(), Some(Control::History(History::Next))),
            ],
        );

        assert!(matches!(
            navigation.control_at(12, 0),
            Some(Control::History(History::Next))
        ));
    }
}
