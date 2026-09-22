use super::{ApplicationMessageSender, Worker};
use review_explore::{ConversationBinding, Exploration, ExplorePass, ViewSave};
use review_thread_service::PinnedAgent;
use std::sync::Arc;

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
        self.open_explore(None, messages);
    }

    pub(super) fn open_explore(
        &mut self,
        instance: Option<String>,
        messages: &ApplicationMessageSender,
    ) {
        let Some(unit) = self.explore.loaded_unit.clone() else {
            return;
        };
        let mut event = ui_events::ExploreRestored {
            result: Ok(None),
            view: None,
            passes: vec![],
            historical: false,
            storage_error: None,
        };
        let result = (|| -> eyre::Result<Option<Arc<ExplorePass>>> {
            let history = self.guide_store.load_explore_history(&unit)?;
            let instance = instance.or_else(|| history.passes.last().cloned());
            event.passes = history.passes;
            let Some(instance) = instance else {
                return Ok(None);
            };
            eyre::ensure!(
                event.passes.contains(&instance),
                "Unknown saved Explore pass"
            );
            event.historical = event.passes.last() != Some(&instance);
            let pass = match self.guide_store.recover_explore_marks(&unit, &instance) {
                Ok(pass) => pass,
                Err(error) => {
                    event.storage_error =
                        Some(format!("Explore file marking needs recovery: {error}"));
                    self.guide_store
                        .load_explore(&unit, &instance)?
                        .ok_or_else(|| {
                            eyre::eyre!("Saved Explore pass is missing; history retained")
                        })?
                }
            };
            event.view = match self.guide_store.load_explore_view(&unit, &instance) {
                Ok(view) => view,
                Err(error) => {
                    event.storage_error = Some(error.to_string());
                    None
                }
            };
            Ok(Some(Arc::new(pass)))
        })();
        match result {
            Ok(pass) => {
                self.explore.prompt = None;
                self.explore.implementation = None;
                self.explore.agent = None;
                self.explore.access = uuid::Uuid::new_v4().to_string();
                self.explore.restored = true;
                self.explore.historical = event.historical;
                self.explore.storage_error.clone_from(&event.storage_error);
                self.explore.pass = pass.as_deref().cloned();
                self.explore.last_view.clone_from(&event.view);
                self.explore.comparison = pass
                    .as_ref()
                    .map(|pass| pass.exploration.comparison.clone());
                event.result = Ok(pass);
            }
            Err(error) => {
                self.explore.storage_error = Some(error.to_string());
                event.result = Err(error.to_string());
            }
        }
        let _ = messages.send(event);
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
}

impl Worker {
    pub(in super::super) fn refresh_explore(&mut self, messages: &ApplicationMessageSender) {
        let Some(previous) = &self.explore.pass else {
            self.open_explore(None, messages);
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
                let _ = messages.send(ui_events::ExploreHistoryChanged(history.passes.clone()));
                self.explore.historical = history.passes.last() != Some(instance);
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
