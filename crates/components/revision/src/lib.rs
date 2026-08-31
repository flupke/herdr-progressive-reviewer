//! Navigation between parent and child revisions.

use component_core::{
    Component, ComponentSubscriptions, EventPublisher, InputMatcher, InputResolution, InputScope,
};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget};
use review_repository::repository::{ChangeId, RevisionCandidate, RevisionDirection};
use ui_actions::Action;
use ui_events::{
    CurrentReviewLocationChanged, RepositoryFilesChanged, RepositoryRefreshFinished,
    RepositoryRefreshStarted, ReviewLocation, ReviewLocationJumped, ReviewLocationRestoreRequested,
    RevisionCandidatesLoaded, RevisionEditFailed, ToastRequested,
};
use ui_shortcuts::{Key, NavigationShortcut, ShortcutCommand, ShortcutMatcher, ShortcutSet};
use ui_theme::Palette;

enum RevisionNavigationState {
    LoadingCandidates {
        direction: RevisionDirection,
        origin: ReviewLocation,
    },
    Selecting {
        candidates: Vec<RevisionCandidate>,
        selected: usize,
        origin: ReviewLocation,
    },
    Editing {
        target_change_id: ChangeId,
        destination: ReviewLocation,
    },
}

/// Revision navigation state, input, and selector rendering.
pub struct RevisionComponent {
    events: EventPublisher,
    palette: Palette,
    current_location: Option<ReviewLocation>,
    pending_repository_refreshes: usize,
    state: Option<RevisionNavigationState>,
}

impl RevisionComponent {
    pub fn new(events: EventPublisher, palette: Palette) -> Self {
        Self {
            events,
            palette,
            current_location: None,
            pending_repository_refreshes: 0,
            state: None,
        }
    }

    pub fn is_modal(&self) -> bool {
        matches!(self.state, Some(RevisionNavigationState::Selecting { .. }))
    }

    pub fn render(&self, area: Rect, buffer: &mut Buffer) {
        let Some(RevisionNavigationState::Selecting {
            candidates,
            selected,
            ..
        }) = &self.state
        else {
            return;
        };
        let width = area.width.saturating_mul(4) / 5;
        let height = u16::try_from(candidates.len())
            .unwrap_or(u16::MAX)
            .saturating_add(2)
            .min(area.height);
        let popup = Rect::new(
            area.x + area.width.saturating_sub(width) / 2,
            area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        );
        Clear.render(popup, buffer);
        let block = Block::default()
            .title("Select revision")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.palette.focus));
        let inner = block.inner(popup);
        block.render(popup, buffer);
        let lines = candidates
            .iter()
            .enumerate()
            .map(|(index, candidate)| {
                let description = if candidate.description.is_empty() {
                    "(no description set)"
                } else {
                    &candidate.description
                };
                let style = if index == *selected {
                    Style::default().bg(self.palette.selection)
                } else {
                    Style::default()
                };
                Line::styled(
                    format!("{}  {description}", candidate.short_change_id),
                    style,
                )
            })
            .collect::<Vec<_>>();
        let visible_rows = usize::from(inner.height).max(1);
        let scroll = selected.saturating_add(1).saturating_sub(visible_rows);
        Paragraph::new(lines)
            .scroll((u16::try_from(scroll).unwrap_or(u16::MAX), 0))
            .render(inner, buffer);
    }

    fn current_location_changed(&mut self, event: &CurrentReviewLocationChanged) {
        self.current_location.clone_from(&event.location);
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn refresh_started(&mut self, _event: &RepositoryRefreshStarted) {
        self.pending_repository_refreshes = self.pending_repository_refreshes.saturating_add(1);
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn refresh_finished(&mut self, _event: &RepositoryRefreshFinished) {
        self.pending_repository_refreshes = self.pending_repository_refreshes.saturating_sub(1);
    }

    fn repository_changed(&mut self, event: &RepositoryFilesChanged) {
        let Some(RevisionNavigationState::Editing {
            target_change_id,
            destination,
        }) = self.state.take()
        else {
            return;
        };
        if target_change_id.review_unit() == &event.review_checkpoint.review_unit {
            self.events.publish(ReviewLocationRestoreRequested {
                location: destination,
            });
        } else {
            self.state = Some(RevisionNavigationState::Editing {
                target_change_id,
                destination,
            });
        }
    }

    fn candidates_loaded(&mut self, event: &RevisionCandidatesLoaded) -> Vec<Action> {
        let Some(RevisionNavigationState::LoadingCandidates { direction, origin }) =
            self.state.take()
        else {
            return Vec::new();
        };
        if direction != event.direction {
            self.state = Some(RevisionNavigationState::LoadingCandidates { direction, origin });
            return Vec::new();
        }
        let candidates = match &event.result {
            Ok(candidates) => candidates.clone(),
            Err(message) => {
                self.events.publish(ToastRequested {
                    text: message.clone(),
                    kind: toasts::ToastKind::Error,
                });
                return Vec::new();
            }
        };
        match candidates.as_slice() {
            [] => Vec::new(),
            [candidate] => vec![self.begin_edit(candidate.clone(), origin)],
            _ => {
                self.state = Some(RevisionNavigationState::Selecting {
                    candidates,
                    selected: 0,
                    origin,
                });
                Vec::new()
            }
        }
    }

    fn edit_failed(&mut self, event: &RevisionEditFailed) {
        self.state = None;
        if let Some(message) = &event.message {
            self.events.publish(ToastRequested {
                text: message.clone(),
                kind: toasts::ToastKind::Error,
            });
        }
    }

    fn keyboard_input(&mut self, input: RevisionInput) -> Vec<Action> {
        let direction = match input {
            RevisionInput::Selector(key) => return self.selector_key(key),
            RevisionInput::Navigate(direction) => direction,
        };
        let Some(origin) = self.current_location.clone() else {
            return Vec::new();
        };
        if self.state.is_some() || self.pending_repository_refreshes > 0 {
            return Vec::new();
        }
        self.state = Some(RevisionNavigationState::LoadingCandidates { direction, origin });
        vec![Action::LoadRevisionCandidates(direction)]
    }

    fn selector_key(&mut self, key: Key) -> Vec<Action> {
        let Some(RevisionNavigationState::Selecting {
            candidates,
            selected,
            origin,
        }) = &mut self.state
        else {
            return Vec::new();
        };
        match key {
            Key::Char('j') | Key::Down => {
                *selected = selected.saturating_add(1).min(candidates.len() - 1);
                Vec::new()
            }
            Key::Char('k') | Key::Up => {
                *selected = selected.saturating_sub(1);
                Vec::new()
            }
            Key::Escape | Key::Char('q') | Key::Quit => {
                self.state = None;
                Vec::new()
            }
            Key::Enter => {
                let candidate = candidates[*selected].clone();
                let origin = origin.clone();
                vec![self.begin_edit(candidate, origin)]
            }
            _ => Vec::new(),
        }
    }

    // Ownership ends the borrow of the active navigation state before this method mutates it.
    #[allow(clippy::needless_pass_by_value)]
    fn begin_edit(&mut self, candidate: RevisionCandidate, origin: ReviewLocation) -> Action {
        let destination = origin.for_review_unit(candidate.change_id.review_unit().clone());
        self.events.publish(ReviewLocationJumped {
            origin: origin.clone(),
            target: destination.clone(),
        });
        self.state = Some(RevisionNavigationState::Editing {
            target_change_id: candidate.change_id.clone(),
            destination,
        });
        Action::EditRevision {
            change_id: candidate.change_id,
        }
    }
}

impl Component<Action> for RevisionComponent {
    fn register_subscriptions(subscriptions: &mut ComponentSubscriptions<'_, Self, Action>) {
        subscriptions.subscribe(Self::current_location_changed);
        subscriptions.subscribe(Self::refresh_started);
        subscriptions.subscribe(Self::refresh_finished);
        subscriptions.subscribe(Self::repository_changed);
        subscriptions.subscribe(Self::candidates_loaded);
        subscriptions.subscribe(Self::edit_failed);
        subscriptions.subscribe_input(
            InputScope::Focused,
            RevisionInputMatcher::new(),
            Self::keyboard_input,
        );
        subscriptions.subscribe_input(
            InputScope::Global,
            RevisionInputMatcher::new(),
            Self::keyboard_input,
        );
    }
}

#[derive(Clone, Copy)]
enum RevisionInput {
    Selector(Key),
    Navigate(RevisionDirection),
}

struct RevisionInputMatcher {
    shortcuts: ShortcutMatcher,
}

impl RevisionInputMatcher {
    const fn new() -> Self {
        Self {
            shortcuts: ShortcutMatcher::new(ShortcutSet::Revision),
        }
    }
}

impl InputMatcher<RevisionComponent, Key> for RevisionInputMatcher {
    type Output = RevisionInput;

    fn resolve(
        &mut self,
        component: &RevisionComponent,
        key: &Key,
    ) -> InputResolution<Self::Output> {
        if component.is_modal() {
            return InputResolution::Matched(RevisionInput::Selector(*key));
        }
        match self.shortcuts.resolve_key(*key) {
            InputResolution::NoMatch => InputResolution::NoMatch,
            InputResolution::AwaitingMoreInput => InputResolution::AwaitingMoreInput,
            InputResolution::Matched(ShortcutCommand::Navigation(
                NavigationShortcut::GoToParentRevision,
            )) => InputResolution::Matched(RevisionInput::Navigate(RevisionDirection::Parents)),
            InputResolution::Matched(ShortcutCommand::Navigation(
                NavigationShortcut::GoToChildRevision,
            )) => InputResolution::Matched(RevisionInput::Navigate(RevisionDirection::Children)),
            InputResolution::Matched(_) => unreachable!("revision shortcut set is exact"),
        }
    }
}

#[cfg(test)]
#[path = "lib.tests.rs"]
mod tests;
