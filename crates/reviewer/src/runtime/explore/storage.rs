use super::{ApplicationMessageSender, Worker};
use review_explore::{ConversationBinding, Exploration, ExplorePass, ViewSave};
use review_thread_service::PinnedAgent;
use std::sync::Arc;

fn empty_restore() -> ui_events::ExploreRestored {
    ui_events::ExploreRestored {
        result: Ok(None),
        view: None,
        historical: false,
        storage_error: None,
    }
}

#[derive(Debug)]
enum ExploreRestoreError {
    Unreadable(UnreadableExplore),
}

#[derive(Debug)]
enum UnreadableExplore {
    History(String),
    Pass { instance: String, reason: String },
}

impl Worker {
    pub(in super::super) fn restore_explore(
        &mut self,
        unit: &review_types::ReviewUnit,
        messages: &ApplicationMessageSender,
    ) {
        if self.explore.loaded_unit.as_ref() == Some(unit) {
            return;
        }
        self.explore = super::ExploreRuntime {
            loaded_unit: Some(unit.clone()),
            restored: true,
            ..Default::default()
        };
        self.open_explore(messages);
    }

    pub(super) fn open_explore(&mut self, messages: &ApplicationMessageSender) {
        let Some(unit) = self.explore.loaded_unit.clone() else {
            return;
        };
        let restored = self.load_explore_for_restore(&unit);
        let (event, toast) = match restored {
            Ok((mut event, toast)) => {
                self.accept_restored_explore(&mut event, messages);
                (event, toast)
            }
            Err(ExploreRestoreError::Unreadable(failure)) => {
                self.discard_unreadable_explore(&unit, failure, messages)
            }
        };
        let _ = messages.send(event);
        if let Some(text) = toast {
            let _ = messages.send(ui_events::ToastRequested {
                text,
                kind: toasts::ToastKind::Error,
            });
        }
    }

    fn load_explore_for_restore(
        &self,
        unit: &review_types::ReviewUnit,
    ) -> Result<(ui_events::ExploreRestored, Option<String>), ExploreRestoreError> {
        let history = self
            .guide_store
            .load_explore_history(unit)
            .map_err(|error| {
                ExploreRestoreError::Unreadable(UnreadableExplore::History(error.to_string()))
            })?;
        let instance = history.passes.last().cloned();
        let Some(instance) = instance else {
            return Ok((empty_restore(), None));
        };
        let historical = history.is_historical(&instance);
        let mut restored = empty_restore();
        let mut toast = None;
        restored.historical = historical;
        let pass = match self.guide_store.recover_explore_marks(unit, &instance) {
            Ok(pass) => pass,
            Err(error) => {
                restored.storage_error =
                    Some(format!("Explore file marking needs recovery: {error}"));
                self.guide_store
                    .load_explore(unit, &instance)
                    .map_err(|error| {
                        ExploreRestoreError::Unreadable(UnreadableExplore::Pass {
                            instance: instance.clone(),
                            reason: error.to_string(),
                        })
                    })?
                    .ok_or_else(|| {
                        ExploreRestoreError::Unreadable(UnreadableExplore::Pass {
                            instance: instance.clone(),
                            reason: "Saved Explore pass is missing".into(),
                        })
                    })?
            }
        };
        restored.view = self.load_explore_view_for_restore(
            unit,
            &instance,
            &mut restored.storage_error,
            &mut toast,
        );
        restored.result = Ok(Some(Arc::new(pass)));
        Ok((restored, toast))
    }

    fn discard_unreadable_explore(
        &mut self,
        unit: &review_types::ReviewUnit,
        failure: UnreadableExplore,
        messages: &ApplicationMessageSender,
    ) -> (ui_events::ExploreRestored, Option<String>) {
        let (reason, cleared) = match failure {
            UnreadableExplore::History(reason) => {
                let result = self.guide_store.repair_explore_history(unit).map(|_| ());
                (reason, result)
            }
            UnreadableExplore::Pass { instance, reason } => {
                let result = self.guide_store.clear_explore_pass(unit, &instance);
                (reason, result)
            }
        };
        let mut event = empty_restore();
        match cleared {
            Ok(()) => {
                self.explore = super::ExploreRuntime {
                    loaded_unit: Some(unit.clone()),
                    restored: true,
                    access: uuid::Uuid::new_v4().to_string(),
                    ..Default::default()
                };
                match self.load_explore_for_restore(unit) {
                    Ok((mut restored, _)) => {
                        self.accept_restored_explore(&mut restored, messages);
                        (
                            restored,
                            Some("Unreadable Explore state was cleared; readable history was retained.".into()),
                        )
                    }
                    Err(error) => {
                        let message = format!(
                            "Explore state could not be restored after clearing unreadable state: {error:?}"
                        );
                        self.explore.storage_error = Some(message.clone());
                        event.result = Err(message);
                        (event, None)
                    }
                }
            }
            Err(clear_error) => {
                let message = format!(
                    "Explore state could not be loaded ({reason}) or cleared ({clear_error})"
                );
                self.explore.storage_error = Some(message.clone());
                event.result = Err(message);
                (event, None)
            }
        }
    }

    fn accept_restored_explore(
        &mut self,
        event: &mut ui_events::ExploreRestored,
        messages: &ApplicationMessageSender,
    ) {
        let pass = event.result.as_ref().ok().and_then(Option::as_ref).cloned();
        self.explore.prompt = None;
        self.explore.implementation = None;
        self.explore.agent = None;
        self.explore.access = uuid::Uuid::new_v4().to_string();
        self.explore.restored = true;
        self.explore.historical = event.historical;
        self.explore.storage_error.clone_from(&event.storage_error);
        self.explore.comparison = pass
            .as_ref()
            .map(|pass| pass.exploration.comparison.clone());
        let pass = pass.map(|pass| {
            if !event.historical && event.storage_error.is_none() {
                Arc::new(self.start_jev_if_enabled((*pass).clone(), messages))
            } else {
                pass
            }
        });
        self.explore.pass = pass.as_deref().cloned();
        self.explore.last_view.clone_from(&event.view);
        event.result = Ok(pass);
    }

    fn load_explore_view_for_restore(
        &self,
        unit: &review_types::ReviewUnit,
        instance: &str,
        storage_error: &mut Option<String>,
        toast: &mut Option<String>,
    ) -> Option<ViewSave> {
        match self.guide_store.load_explore_view(unit, instance) {
            Ok(view) => view,
            Err(error) => {
                match self.guide_store.clear_explore_view(unit, instance) {
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

    pub(super) fn save_explore_view(
        &mut self,
        view: ViewSave,
        messages: &ApplicationMessageSender,
    ) {
        if self.explore.storage_error.is_some() {
            return;
        }
        let unit = view.review_unit.clone();
        let result = self.guide_store.save_explore_view(&unit, &view);
        if self.explore.loaded_unit.as_ref() == Some(&unit) {
            self.explore.last_view = Some(view);
        }
        if let Err(error) = result {
            self.explore.storage_error = Some(error.to_string());
            let _ = messages.send(ui_events::ExploreStorageFailed(error.to_string()));
        }
    }

    pub(super) fn persist_explore_request(
        &mut self,
        request: &review_explore::TurnRequest,
        retry_agent: Option<&herdr_client::protocol::Agent>,
    ) -> eyre::Result<ExplorePass> {
        eyre::ensure!(
            self.explore.storage_error.is_none(),
            "{}",
            self.explore.storage_error.as_deref().unwrap_or_default()
        );
        eyre::ensure!(
            !self.explore.historical,
            "This pass is history; open the latest pass or start a New pass"
        );
        if self.explore.pass.is_none() {
            eyre::ensure!(retry_agent.is_none(), "No Explore pass to retry");
            let mut exploration = Exploration::new(
                self.explore
                    .comparison
                    .clone()
                    .ok_or_else(|| eyre::eyre!("Start Explore first"))?,
            );
            exploration.instance.clone_from(&request.instance);
            let mut pass = ExplorePass::new(exploration);
            pass.post(request)?;
            pass.binding = self
                .explore
                .agent
                .as_ref()
                .and_then(PinnedAgent::known_agent)
                .as_ref()
                .and_then(ConversationBinding::from_agent);
            self.explore.loaded_unit = Some(request.checkpoint.review_unit.clone());
            return Ok(self.guide_store.create_explore(pass)?);
        }
        Ok(self
            .guide_store
            .update_explore(&request.checkpoint.review_unit, &request.instance, |pass| {
                let new = pass.post(request).map_err(|e| e.to_string())?;
                if let Some(agent) = retry_agent {
                    if let Some(previous) = &pass.binding
                        && !previous.same_agent_kind(agent)
                    {
                        return Err("The selected pane is running a different agent".into());
                    }
                    pass.binding = ConversationBinding::from_agent(agent);
                }
                if new
                    && let Some(view) = &self.explore.last_view
                    && view.instance == request.instance
                {
                    pass.turns
                        .get_mut(&request.request)
                        .expect("posted turn")
                        .editor_sequence = Some(view.sequence);
                }
                Ok(new)
            })?
            .1)
    }

    pub(super) fn bound_explore_agent(&mut self) -> eyre::Result<PinnedAgent> {
        if let Some(agent) = &self.explore.agent {
            agent.current(&self.client).map_err(eyre::Report::msg)?;
            return Ok(agent.clone());
        }
        let selected = self.target.resolve(&self.client)?.ok_or_else(|| {
            eyre::eyre!(
                "Original implementation agent is unavailable. History and edits remain available."
            )
        })?;
        if let Some(binding) = self
            .explore
            .pass
            .as_ref()
            .and_then(|pass| pass.binding.as_ref())
        {
            eyre::ensure!(
                selected.agent_session.is_some(),
                "Waiting for the native agent conversation identity"
            );
            eyre::ensure!(
                binding.matches(&selected),
                "Different agent conversation. Return to the original conversation or start a New pass."
            );
        } else {
            eyre::ensure!(
                !self.explore.restored,
                "The original conversation binding was never established. Start a New pass to use this agent."
            );
        }
        let agent = PinnedAgent::new(selected);
        self.explore.agent = Some(agent.clone());
        Ok(agent)
    }

    pub(super) fn retry_explore_agent(&mut self) -> eyre::Result<herdr_client::protocol::Agent> {
        let selected = if let Some(agent) = &self.explore.agent {
            agent
                .retry_target(&self.client)
                .map_err(eyre::Report::msg)?
        } else {
            self.target.resolve(&self.client)?
        }
        .ok_or_else(|| eyre::eyre!("Selected implementation agent is unavailable"))?;
        eyre::ensure!(
            selected.agent_session.is_some(),
            "Waiting for the native agent conversation identity"
        );
        if let Some(binding) = self
            .explore
            .pass
            .as_ref()
            .and_then(|pass| pass.binding.as_ref())
        {
            eyre::ensure!(
                binding.same_agent_kind(&selected),
                "The selected pane is running a different agent"
            );
        }
        Ok(selected)
    }
}

impl Worker {
    pub(in super::super) fn refresh_explore(&mut self, messages: &ApplicationMessageSender) {
        let Some(previous) = &self.explore.pass else {
            self.open_explore(messages);
            return;
        };
        let unit = &previous.exploration.comparison.checkpoint.review_unit;
        let instance = &previous.exploration.instance;
        let result = (|| -> eyre::Result<_> {
            let history = self.guide_store.load_explore_history(unit)?;
            let pass = self
                .guide_store
                .load_explore(unit, instance)?
                .ok_or_else(|| eyre::eyre!("Saved Explore pass disappeared"))?;
            Ok((history, pass))
        })();
        match result {
            Ok((history, pass)) => {
                let _ = messages.send(ui_events::ExploreHistoryChanged(history.clone()));
                self.explore.historical = history.is_historical(instance);
                if pass.revision <= previous.revision {
                    return;
                }
                let (response, _) = std::sync::mpsc::channel();
                let _ = messages.send(ui_events::ExploreCommitted {
                    pass: Arc::new(pass.clone()),
                    applied: true,
                    response,
                });
                self.explore.pass = Some(pass);
            }
            Err(error) => {
                self.explore.storage_error = Some(error.to_string());
                let _ = messages.send(ui_events::ExploreStorageFailed(error.to_string()));
            }
        }
    }
}
