//! Saved round restoration, editor views and adoption of changes saved elsewhere.

use std::sync::Arc;

use review_explore::{DispatchState, ExploreRound, ViewSave};
use review_types::ReviewUnit;

use crate::{ExploreSession, State, publish_committed};

fn empty_restore() -> ui_events::ExploreRestored {
    ui_events::ExploreRestored {
        result: Ok(None),
        view: None,
        historical: false,
        storage_error: None,
        progress: ui_events::ExploreProgress::Ready,
    }
}

/// Where a restored round's interview stands. A turn whose prompt attempt began may
/// have reached the agent even though no outcome was saved.
fn restored_progress(round: &ExploreRound) -> ui_events::ExploreProgress {
    let Some(request) = round.exploration.retry_request() else {
        return ui_events::ExploreProgress::Ready;
    };
    let uncertain = round.turns.get(&request.request).is_some_and(|delivery| {
        matches!(
            delivery.state,
            DispatchState::Attempting | DispatchState::Unknown
        )
    });
    if uncertain {
        ui_events::ExploreProgress::DeliveryUncertain
    } else {
        ui_events::ExploreProgress::Interrupted
    }
}

#[derive(Debug)]
enum RestoreError {
    Unreadable(Unreadable),
}

#[derive(Debug)]
enum Unreadable {
    History(String),
    Round { instance: String, reason: String },
}

impl ExploreSession {
    pub(crate) fn open(&mut self) {
        let Some(unit) = self.state.loaded_unit.clone() else {
            return;
        };
        let restored = self.load_for_restore(&unit);
        let (event, toast) = match restored {
            Ok((mut event, toast)) => {
                self.accept_restored(&mut event);
                (event, toast)
            }
            Err(RestoreError::Unreadable(failure)) => self.discard_unreadable(&unit, failure),
        };
        let _ = self.events.send(event);
        if let Some(text) = toast {
            let _ = self.events.send(ui_events::ToastRequested {
                text,
                kind: toasts::ToastKind::Error,
            });
        }
    }

    fn load_for_restore(
        &self,
        unit: &ReviewUnit,
    ) -> Result<(ui_events::ExploreRestored, Option<String>), RestoreError> {
        let history = self
            .rounds
            .history(unit)
            .map_err(|error| RestoreError::Unreadable(Unreadable::History(error.to_string())))?;
        let instance = history.rounds.last().cloned();
        let Some(instance) = instance else {
            return Ok((empty_restore(), None));
        };
        let historical = history.is_historical(&instance);
        let mut restored = empty_restore();
        let mut toast = None;
        restored.historical = historical;
        let round = self
            .rounds
            .round(unit, &instance)
            .map_err(|error| {
                RestoreError::Unreadable(Unreadable::Round {
                    instance: instance.clone(),
                    reason: error.to_string(),
                })
            })?
            .ok_or_else(|| {
                RestoreError::Unreadable(Unreadable::Round {
                    instance: instance.clone(),
                    reason: "Saved Explore round is missing".into(),
                })
            })?;
        restored.view =
            self.load_view_for_restore(unit, &instance, &mut restored.storage_error, &mut toast);
        restored.progress = restored_progress(&round);
        restored.result = Ok(Some(Arc::new(round)));
        Ok((restored, toast))
    }

    fn discard_unreadable(
        &mut self,
        unit: &ReviewUnit,
        failure: Unreadable,
    ) -> (ui_events::ExploreRestored, Option<String>) {
        let (reason, cleared) = match failure {
            Unreadable::History(reason) => {
                let result = self.rounds.repair_history(unit).map(|_| ());
                (reason, result)
            }
            Unreadable::Round { instance, reason } => {
                let result = self.rounds.clear_round(unit, &instance);
                (reason, result)
            }
        };
        let mut event = empty_restore();
        match cleared {
            Ok(()) => {
                self.state = State {
                    loaded_unit: Some(unit.clone()),
                    ..State::default()
                };
                self.state.renew_access();
                match self.load_for_restore(unit) {
                    Ok((mut restored, _)) => {
                        self.accept_restored(&mut restored);
                        (
                            restored,
                            Some("Unreadable Explore state was cleared; readable history was retained.".into()),
                        )
                    }
                    Err(error) => {
                        let message = format!(
                            "Explore state could not be restored after clearing unreadable state: {error:?}"
                        );
                        self.state.storage_error = Some(message.clone());
                        event.result = Err(message);
                        (event, None)
                    }
                }
            }
            Err(clear_error) => {
                let message = format!(
                    "Explore state could not be loaded ({reason}) or cleared ({clear_error})"
                );
                self.state.storage_error = Some(message.clone());
                event.result = Err(message);
                (event, None)
            }
        }
    }

    fn accept_restored(&mut self, event: &mut ui_events::ExploreRestored) {
        let round = event.result.as_ref().ok().and_then(Option::as_ref).cloned();
        self.state.prompt = None;
        self.state.implementation = None;
        self.state.agent = None;
        self.state.renew_access();
        self.state.historical = event.historical;
        self.state.storage_error.clone_from(&event.storage_error);
        self.state.comparison = round
            .as_ref()
            .map(|round| round.exploration.comparison.clone());
        self.state.round = round.as_deref().cloned();
        self.state.last_view.clone_from(&event.view);
        event.result = Ok(round);
    }

    fn load_view_for_restore(
        &self,
        unit: &ReviewUnit,
        instance: &str,
        storage_error: &mut Option<String>,
        toast: &mut Option<String>,
    ) -> Option<ViewSave> {
        match self.rounds.view(unit, instance) {
            Ok(view) => view,
            Err(error) => {
                match self.rounds.clear_view(unit, instance) {
                    Ok(()) => {
                        *toast = Some("Unreadable Explore editor state was cleared.".to_owned());
                    }
                    Err(clear_error) => {
                        *storage_error = Some(format!(
                            "Explore editor state could not be loaded ({error}) or cleared ({clear_error})"
                        ));
                    }
                }
                None
            }
        }
    }

    pub(crate) fn save_view(&mut self, view: ViewSave) {
        if self.state.storage_error.is_some() {
            return;
        }
        let unit = view.review_unit.clone();
        let result = self.rounds.save_view(&unit, &view);
        if self.state.loaded_unit.as_ref() == Some(&unit) {
            self.state.last_view = Some(view);
        }
        if let Err(error) = result {
            self.state.storage_error = Some(error.to_string());
            let _ = self
                .events
                .send(ui_events::ExploreStorageFailed(error.to_string()));
        }
    }

    /// Saved Explore state changed on disk; adopt a newer revision of the current round.
    pub(crate) fn storage_changed(&mut self) {
        let Some(previous) = &self.state.round else {
            self.open();
            return;
        };
        let unit = &previous.exploration.comparison.checkpoint.review_unit;
        let instance = &previous.exploration.instance;
        let result = (|| -> eyre::Result<_> {
            let history = self.rounds.history(unit)?;
            let round = self
                .rounds
                .round(unit, instance)?
                .ok_or_else(|| eyre::eyre!("Saved Explore round disappeared"))?;
            Ok((history, round))
        })();
        match result {
            Ok((history, round)) => {
                let _ = self
                    .events
                    .send(ui_events::ExploreHistoryChanged(history.clone()));
                self.state.historical = history.is_historical(instance);
                if round.revision <= previous.revision {
                    return;
                }
                publish_committed(&self.events, round.clone());
                self.state.round = Some(round);
            }
            Err(error) => {
                self.state.storage_error = Some(error.to_string());
                let _ = self
                    .events
                    .send(ui_events::ExploreStorageFailed(error.to_string()));
            }
        }
    }
}
