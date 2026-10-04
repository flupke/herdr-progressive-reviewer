use super::{
    ComposeScope, Control, ExploreComponent,
    controls::{Button, ControlVisual},
};
use ratatui::{buffer::Buffer, layout::Rect};
use ui_controls::label_width;
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

/// Navigation bar labels, each with the control a click activates.
pub(super) type Labels = Vec<(String, Option<Control>)>;

/// History controls stay visible while the question and its evidence scroll.
#[derive(Clone, Default)]
pub(super) struct Navigation {
    items: Vec<Item>,
    height: u16,
}

impl Navigation {
    /// Lay out `labels` from the left and `corner` in the top-right corner.
    fn new(area: Rect, labels: Labels, corner: Labels) -> Self {
        let mut result = Self::default();
        let width = area.width.saturating_sub(2);
        let visuals = |labels: Labels| {
            labels
                .into_iter()
                .map(|(label, control)| (ControlVisual::Text(label), control))
        };
        // The corner's controls are buttons. A corner that does not fit beside the
        // first label keeps only them.
        let corner: Vec<_> = corner
            .into_iter()
            .map(|(label, control)| {
                let visual = control.map_or_else(
                    || ControlVisual::Text(label.clone()),
                    |control| control.visual(label.clone()),
                );
                (visual, control)
            })
            .collect();
        let span = corner.iter().fold(0_u16, |span, (visual, _)| {
            span.saturating_add(label_width(&visual.text()))
                .saturating_add(1)
        });
        let first = labels
            .first()
            .map_or(0, |(label, _)| label_width(label).saturating_add(1));
        let fitting = corner.into_iter().filter(|(_, control)| {
            control.is_some() || first.saturating_add(span) <= width.saturating_add(1)
        });
        let corner = Button::wrap_right(width, fitting)
            .into_iter()
            .next()
            .unwrap_or_default();
        let reserved = corner
            .first()
            .map_or(0, |button| width.saturating_sub(button.column) + 1);
        let mut rows = Button::wrap(width.saturating_sub(reserved), visuals(labels));
        if !corner.is_empty() {
            if rows.is_empty() {
                rows.push(Vec::new());
            }
            rows[0].extend(corner);
        }
        for (row, buttons) in rows.into_iter().enumerate().take(usize::from(area.height)) {
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
        if self.shows_page_round() {
            // The questions are on the page: only Reset stays.
            return Navigation::new(area, Vec::new(), self.reset_controls());
        }
        let history = HistoryPages::new(self);
        let Some(last) = history.pages.len().checked_sub(1) else {
            return Navigation::new(area, self.execution_controls(), self.reset_controls());
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
        Navigation::new(area, labels, self.reset_controls())
    }

    fn execution_controls(&self) -> Labels {
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
    fn a_corner_too_wide_for_the_pane_keeps_its_control() {
        let navigation = Navigation::new(
            Rect::new(0, 0, 30, 3),
            vec![("Question 1/1".into(), None)],
            vec![
                ("Reset closes this round for good.".into(), None),
                ("Confirm reset".into(), Some(Control::ConfirmReset)),
            ],
        );

        assert_eq!(navigation.height(), 1);
        assert!(matches!(
            navigation.control_at(20, 0),
            Some(Control::ConfirmReset)
        ));
    }

    #[test]
    fn a_corner_keeps_clear_of_the_first_label() {
        let navigation = Navigation::new(
            Rect::new(0, 0, 60, 3),
            vec![("Question 1/1 Agent total 0.3s".into(), None)],
            vec![
                ("Reset closes this round for good.".into(), None),
                ("Confirm reset".into(), Some(Control::ConfirmReset)),
            ],
        );

        let label = &navigation.items[0];
        let corner = &navigation.items[1];
        assert_eq!(navigation.items.len(), 2, "the corner's text is dropped");
        assert!(label.area.right() < corner.area.left());
        assert!(matches!(corner.button.control, Some(Control::ConfirmReset)));
    }

    #[test]
    fn pinned_controls_use_displayed_width_for_unicode_clicks() {
        let navigation = Navigation::new(
            Rect::new(0, 0, 40, 1),
            vec![
                ("Map view ▾".into(), Some(Control::Map)),
                ("Next".into(), Some(Control::History(History::Next))),
            ],
            Vec::new(),
        );

        assert!(matches!(
            navigation.control_at(12, 0),
            Some(Control::History(History::Next))
        ));
    }
}
