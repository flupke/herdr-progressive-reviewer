use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::Widget;
use review_repository::repository::{RevisionCandidate, RevisionDirection};
use toasts::ToastKind;

use crate::app::{
    Action, ActivePopup, Key, ReviewApp, RevisionNavigationCompletion, RevisionNavigationState,
};
use crate::navigation::{LocationHistory, ReviewLocation};
use crate::popup::PopupView;

pub(super) struct RevisionSelectorView<'a>(pub(super) &'a ReviewApp);

impl Widget for RevisionSelectorView<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let Some(RevisionNavigationState::Selecting {
            candidates,
            selected,
            ..
        }) = &self.0.revision_navigation
        else {
            return;
        };
        let width = area.width.saturating_mul(4) / 5;
        let height = u16::try_from(candidates.len())
            .unwrap_or(u16::MAX)
            .saturating_add(2)
            .min(area.height.saturating_mul(4) / 5);
        let popup = Rect::new(
            area.x + area.width.saturating_sub(width) / 2,
            area.y + area.height.saturating_sub(height) / 2,
            width,
            height,
        );
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
                    Style::default().bg(self.0.palette.selection)
                } else {
                    Style::default()
                };
                Line::styled(
                    format!("{}  {description}", candidate.short_change_id),
                    style,
                )
            })
            .collect::<Vec<_>>();
        let visible_rows = usize::from(popup.height.saturating_sub(2)).max(1);
        let scroll = selected.saturating_add(1).saturating_sub(visible_rows);
        PopupView::new(self.0, "Select revision", lines)
            .scroll(u16::try_from(scroll).unwrap_or(u16::MAX))
            .render(popup, buffer);
    }
}

impl ReviewApp {
    pub(super) fn revision_location_waits_for(&self, snapshot_id: &str, path: &str) -> bool {
        if self.commit_id != snapshot_id {
            return false;
        }
        matches!(
            &self.revision_navigation,
            Some(RevisionNavigationState::Editing { destination, .. })
                if match destination {
                    ReviewLocation::ReviewFile {
                        path: destination_path,
                        ..
                    } => destination_path == path,
                    ReviewLocation::Source { location, .. } => location
                        .review_path(&self.repository_root)
                        .is_some_and(|destination_path| destination_path == path),
                    ReviewLocation::Revision { .. } => false,
                }
        )
    }

    pub(super) fn revision_source_restore_is_pending(&self, snapshot_id: &str) -> bool {
        self.commit_id == snapshot_id
            && matches!(
                &self.revision_navigation,
                Some(RevisionNavigationState::Editing {
                    destination: ReviewLocation::Source { .. },
                    ..
                })
            )
    }

    pub(super) fn revision_source_waits_for(&self, location: &review_lsp::SourceLocation) -> bool {
        matches!(
            &self.revision_navigation,
            Some(RevisionNavigationState::Editing {
                destination: ReviewLocation::Source {
                    location: destination,
                    ..
                },
                ..
            }) if destination.path == location.path
        )
    }

    pub(super) fn start_revision_navigation(&mut self, direction: RevisionDirection) -> Action {
        if self.revision_navigation.is_some() || self.pending_repository_refreshes > 0 {
            return Action::None;
        }
        let Some(origin) = self.current_review_location() else {
            return Action::None;
        };
        self.revision_navigation =
            Some(RevisionNavigationState::LoadingCandidates { direction, origin });
        Action::LoadRevisionCandidates(direction)
    }

    pub(super) fn load_revision_candidates(
        &mut self,
        direction: RevisionDirection,
        result: Result<Vec<RevisionCandidate>, String>,
    ) -> Action {
        let Some(RevisionNavigationState::LoadingCandidates {
            direction: pending_direction,
            origin,
        }) = self.revision_navigation.take()
        else {
            return Action::None;
        };
        if direction != pending_direction {
            self.revision_navigation = Some(RevisionNavigationState::LoadingCandidates {
                direction: pending_direction,
                origin,
            });
            return Action::None;
        }
        let candidates = match result {
            Ok(candidates) => candidates,
            Err(message) => {
                self.toasts.push(message, ToastKind::Error);
                return Action::None;
            }
        };
        match candidates.as_slice() {
            [] => Action::None,
            [candidate] => self.begin_revision_edit(candidate.clone(), origin),
            _ => {
                self.active_popup = Some(ActivePopup::RevisionSelector);
                self.revision_navigation = Some(RevisionNavigationState::Selecting {
                    candidates,
                    selected: 0,
                    origin,
                });
                Action::None
            }
        }
    }

    pub(super) fn revision_selector_key(&mut self, key: Key) -> Action {
        let Some(RevisionNavigationState::Selecting {
            candidates,
            selected,
            origin,
        }) = &mut self.revision_navigation
        else {
            self.active_popup = None;
            return Action::None;
        };
        match key {
            Key::Char('j') | Key::Down => {
                *selected = selected.saturating_add(1).min(candidates.len() - 1);
                Action::None
            }
            Key::Char('k') | Key::Up => {
                *selected = selected.saturating_sub(1);
                Action::None
            }
            Key::Escape | Key::Char('q') | Key::Quit => {
                self.active_popup = None;
                self.revision_navigation = None;
                Action::None
            }
            Key::Enter => {
                let candidate = candidates[*selected].clone();
                let origin = origin.clone();
                self.active_popup = None;
                self.revision_navigation = None;
                self.begin_revision_edit(candidate, origin)
            }
            _ => Action::None,
        }
    }

    fn begin_revision_edit(
        &mut self,
        candidate: RevisionCandidate,
        origin: ReviewLocation,
    ) -> Action {
        let destination = origin.for_review_unit(candidate.change_id.review_unit().clone());
        self.revision_navigation = Some(RevisionNavigationState::Editing {
            target_change_id: candidate.change_id.clone(),
            destination,
            completion: RevisionNavigationCompletion::RecordJump { origin },
        });
        Action::EditRevision {
            change_id: candidate.change_id,
        }
    }

    pub(super) fn edit_historical_revision(
        &mut self,
        destination: ReviewLocation,
        history_before: LocationHistory,
    ) -> Action {
        if self.revision_navigation.is_some() {
            self.location_history = history_before;
            return Action::None;
        }
        let target_change_id =
            review_repository::repository::ChangeId::from(destination.review_unit());
        self.revision_navigation = Some(RevisionNavigationState::Editing {
            target_change_id: target_change_id.clone(),
            destination,
            completion: RevisionNavigationCompletion::RestoreHistory { history_before },
        });
        Action::EditRevision {
            change_id: target_change_id,
        }
    }

    pub(super) fn fail_revision_edit(&mut self, message: Option<String>) -> Action {
        if let Some(RevisionNavigationState::Editing {
            completion: RevisionNavigationCompletion::RestoreHistory { history_before },
            ..
        }) = self.revision_navigation.take()
        {
            self.location_history = history_before;
        }
        if let Some(message) = message {
            self.toasts.push(message, ToastKind::Error);
        }
        Action::None
    }

    pub(super) fn complete_revision_edit(&mut self) -> Action {
        let Some(RevisionNavigationState::Editing {
            target_change_id,
            destination,
            completion,
        }) = self.revision_navigation.take()
        else {
            return self.load_selected_action();
        };
        if target_change_id.review_unit() != &self.review_unit {
            self.revision_navigation = Some(RevisionNavigationState::Editing {
                target_change_id,
                destination,
                completion,
            });
            return self.load_selected_action();
        }
        let action = self.show_review_location(destination.clone());
        if matches!(action, Action::LoadDiff { .. } | Action::LoadSource { .. }) {
            self.revision_navigation = Some(RevisionNavigationState::Editing {
                target_change_id,
                destination,
                completion,
            });
            return action;
        }
        if let RevisionNavigationCompletion::RecordJump { origin } = completion {
            let _ = self.record_location_change(Some(origin));
        }
        action
    }

    pub(super) fn finish_revision_location_restore(&mut self) {
        let Some(RevisionNavigationState::Editing { completion, .. }) =
            self.revision_navigation.take()
        else {
            return;
        };
        if let RevisionNavigationCompletion::RecordJump { origin } = completion {
            let _ = self.record_location_change(Some(origin));
        }
    }
}

#[cfg(test)]
#[path = "revision_navigation.tests.rs"]
mod tests;
