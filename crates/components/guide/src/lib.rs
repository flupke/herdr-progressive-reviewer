//! Review-guide state and input handling.

use component_core::{Component, ComponentSubscriptions, EventPublisher, InputScope};
use guide_rendering::{GuideLayout, GuideOverlay};
use ratatui::buffer::Buffer;
use ratatui::style::Color;
use review_guide::{GuideItem, GuideScope, ReviewCheckpoint};
use ui_actions::Action;
use ui_events::{
    AnimationTick, DisplayedDiffViewport, DisplayedDiffViewportsChanged, FileSelected,
    GuideCounter, GuideJumpRequested, GuideLayoutChanged, GuidePathsChanged,
    RepositoryFilesChanged, ReviewGuideChanged, ReviewGuideStatusChanged,
};
use ui_shortcuts::{GuideShortcut, ShortcutCommand, ShortcutMatcher, ShortcutSet};

#[derive(Clone, Copy)]
struct GuidePosition {
    item_index: usize,
    file_index: usize,
    row: Option<usize>,
}

/// Guide state and guide-owned input behavior.
pub struct GuideComponent {
    events: EventPublisher,
    review_checkpoint: Option<ReviewCheckpoint>,
    selected_path: Option<String>,
    items: Vec<GuideItem>,
    viewports: Vec<DisplayedDiffViewport>,
    positions: Vec<GuidePosition>,
    counters: Vec<Option<GuideCounter>>,
    current_file_index: usize,
    current_row: usize,
    spinner_frame: Option<usize>,
}

impl GuideComponent {
    /// Create an empty guide component.
    pub fn new(events: EventPublisher) -> Self {
        Self {
            events,
            review_checkpoint: None,
            selected_path: None,
            items: Vec::new(),
            viewports: Vec::new(),
            positions: Vec::new(),
            counters: Vec::new(),
            current_file_index: 0,
            current_row: 0,
            spinner_frame: None,
        }
    }

    /// Build the guide layout for one displayed diff viewport.
    pub fn layout<'a>(
        &'a self,
        viewport: &DisplayedDiffViewport,
        guide_color: Color,
    ) -> GuideLayout<'a> {
        GuideLayout::new(
            &self.items,
            &self.counters,
            viewport.rows.len(),
            guide_color,
            |target| guide_rendering::target_rows(viewport, target),
        )
    }

    /// Draw the positioned guide layer after the diff surface.
    pub fn render(&self, overlay: &GuideOverlay, buffer: &mut Buffer) {
        if !self.items.is_empty() {
            overlay.render(buffer);
        }
    }

    fn viewports_changed(&mut self, event: &DisplayedDiffViewportsChanged) {
        self.viewports.clone_from(&event.viewports);
        self.current_file_index = event.current_file_index;
        self.current_row = event.current_row;
        self.rebuild_positions();
    }

    fn repository_changed(&mut self, event: &RepositoryFilesChanged) {
        let same_review_unit = self.review_checkpoint.as_ref().is_some_and(|checkpoint| {
            checkpoint.review_unit == event.review_checkpoint.review_unit
        });
        let same_checkpoint = self.review_checkpoint.as_ref() == Some(&event.review_checkpoint);
        if !same_review_unit {
            self.items.clear();
            self.positions.clear();
            self.counters.clear();
            self.publish_layout();
        }
        if !same_checkpoint {
            self.spinner_frame = None;
        }
        self.review_checkpoint = Some(event.review_checkpoint.clone());
    }

    fn file_selected(&mut self, event: &FileSelected) {
        self.selected_path = Some(event.path.clone());
    }

    fn status_changed(&mut self, event: &ReviewGuideStatusChanged) {
        if self.matches(&event.review_checkpoint) {
            self.spinner_frame = event.generating.then_some(0);
        }
    }

    fn guide_changed(&mut self, event: &ReviewGuideChanged) {
        if !self.matches(&event.review_checkpoint) {
            return;
        }
        self.items.clone_from(&event.items);
        self.rebuild_positions();
        self.spinner_frame = None;
        self.events.publish(GuidePathsChanged {
            paths: self
                .items
                .iter()
                .map(|item| item.target.path().to_owned())
                .collect(),
        });
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn tick(&mut self, _event: &AnimationTick) {
        if let Some(frame) = &mut self.spinner_frame {
            *frame = frame.saturating_add(1);
        }
    }

    fn run_shortcut(&mut self, shortcut: ShortcutCommand) -> Vec<Action> {
        let ShortcutCommand::Guide(shortcut) = shortcut else {
            return Vec::new();
        };
        let action = match shortcut {
            GuideShortcut::GenerateSelectedFile => {
                let Some(path) = self.selected_path.clone() else {
                    return Vec::new();
                };
                (self.spinner_frame.is_none()).then_some(Action::GenerateReviewGuide {
                    scope: GuideScope::File { path },
                })
            }
            GuideShortcut::GenerateFull => {
                (self.spinner_frame.is_none()).then_some(Action::GenerateReviewGuide {
                    scope: GuideScope::All,
                })
            }
            GuideShortcut::GoToPreviousComment | GuideShortcut::GoToNextComment => {
                self.navigate(shortcut);
                None
            }
        };
        action.into_iter().collect()
    }

    fn matches(&self, checkpoint: &review_guide::ReviewCheckpoint) -> bool {
        self.review_checkpoint.as_ref() == Some(checkpoint)
    }

    fn rebuild_positions(&mut self) {
        self.positions = self
            .viewports
            .iter()
            .flat_map(|viewport| {
                self.items
                    .iter()
                    .enumerate()
                    .filter_map(move |(item_index, item)| {
                        if item.target.path() != viewport.path {
                            return None;
                        }
                        let row = if viewport.rows.is_empty() && !viewport.can_show_file {
                            None
                        } else {
                            Some(target_rows(viewport, &item.target)?.0)
                        };
                        Some(GuidePosition {
                            item_index,
                            file_index: viewport.file_index,
                            row,
                        })
                    })
            })
            .collect();
        self.positions
            .sort_by_key(|position| (position.file_index, position.row.unwrap_or(usize::MAX)));
        let total = self.positions.len();
        self.counters = vec![None; self.items.len()];
        for (offset, position) in self.positions.iter().enumerate() {
            if let Some(counter) = self.counters.get_mut(position.item_index) {
                *counter = Some(GuideCounter {
                    number: offset.saturating_add(1),
                    total,
                });
            }
        }
        self.publish_layout();
    }

    fn publish_layout(&self) {
        self.events.publish(GuideLayoutChanged {
            items: self.items.clone(),
            counters: self.counters.clone(),
        });
    }

    fn navigate(&self, shortcut: GuideShortcut) {
        let current = (self.current_file_index, self.current_row);
        let target = match shortcut {
            GuideShortcut::GoToNextComment => self
                .positions
                .iter()
                .find(|position| position_is_after(**position, current))
                .or_else(|| self.positions.first()),
            GuideShortcut::GoToPreviousComment => self
                .positions
                .iter()
                .rev()
                .find(|position| position_is_before(**position, current))
                .or_else(|| self.positions.last()),
            GuideShortcut::GenerateSelectedFile | GuideShortcut::GenerateFull => None,
        };
        let Some(position) = target else {
            return;
        };
        let Some(item) = self.items.get(position.item_index) else {
            return;
        };
        self.events.publish(GuideJumpRequested {
            file_index: position.file_index,
            row: position.row,
            target: item.target.clone(),
        });
    }
}

fn position_is_after(position: GuidePosition, current: (usize, usize)) -> bool {
    position.file_index > current.0
        || (position.file_index == current.0 && position.row.is_some_and(|row| row > current.1))
}

fn position_is_before(position: GuidePosition, current: (usize, usize)) -> bool {
    position.file_index < current.0
        || (position.file_index == current.0 && position.row.is_some_and(|row| row < current.1))
}

impl Component<Action> for GuideComponent {
    fn register_subscriptions(subscriptions: &mut ComponentSubscriptions<'_, Self, Action>) {
        subscriptions.subscribe(Self::repository_changed);
        subscriptions.subscribe(Self::file_selected);
        subscriptions.subscribe(Self::status_changed);
        subscriptions.subscribe(Self::guide_changed);
        subscriptions.subscribe(Self::viewports_changed);
        subscriptions.subscribe(Self::tick);
        subscriptions.subscribe_input(
            InputScope::Global,
            ShortcutMatcher::new(ShortcutSet::Guide),
            Self::run_shortcut,
        );
    }
}

fn target_rows(
    viewport: &DisplayedDiffViewport,
    target: &review_guide::GuideTarget,
) -> Option<(usize, usize)> {
    guide_rendering::target_rows(viewport, target)
}

#[cfg(test)]
#[path = "lib.tests.rs"]
mod tests;
