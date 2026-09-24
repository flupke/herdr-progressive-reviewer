//! Durable Explore commands using the shared dispatcher and authoritative UI acknowledgement.
use super::{ApplicationMessageSender, Worker, WorkerCommand};
use review_explore::{Command, Comparison, TurnRequest};
use review_thread_service::{PinnedAgent, PromptCancellation};
use std::sync::Arc;
mod dispatch;
mod implementation;
mod jev;
mod storage;

#[derive(Debug, Default)]
pub(super) struct ExploreRuntime {
    comparison: Option<Arc<Comparison>>,
    pass: Option<review_explore::ExplorePass>,
    loaded_unit: Option<review_types::ReviewUnit>,
    restored: bool,
    historical: bool,
    storage_error: Option<String>,
    access: String,
    last_view: Option<review_explore::ViewSave>,
    pending: Option<(String, String)>,
    agent: Option<PinnedAgent>,
    prompt: Option<PromptCancellation>,
    implementation: Option<PromptCancellation>,
    jev_active: Option<(String, Arc<std::sync::atomic::AtomicBool>)>,
}

impl Worker {
    pub(super) fn explore_command(
        &mut self,
        command: Command,
        messages: &ApplicationMessageSender,
    ) {
        match command {
            Command::Start => self.start_explore(messages),
            Command::SaveView(view) => self.save_explore_view(*view, messages),
            Command::OpenPass(instance) => self.open_explore(Some(instance), messages),
            Command::Turn(request) => self.explore_turn(*request, messages),
            Command::Implement(request) => self.implement_explore(request, messages),
            Command::RequireReview(units) => self.require_explore_review(*units, messages),
            Command::CancelImplementation => self.explore.implementation = None,
            Command::Cancel => {
                self.cancel_explore_record(messages);
                self.explore.access = uuid::Uuid::new_v4().to_string();
                self.explore.prompt = None;
                self.explore.pending = None;
                self.explore.implementation = None;
            }
        }
    }

    fn require_explore_review(
        &mut self,
        units: Vec<review_explore::CoverageUnit>,
        messages: &ApplicationMessageSender,
    ) {
        let Some(pass) = &self.explore.pass else {
            return;
        };
        if self.explore.historical || pass.completion.is_some() {
            return;
        }
        let unit = pass.exploration.comparison.checkpoint.review_unit.clone();
        let instance = pass.exploration.instance.clone();
        let result = self.guide_store.update_explore(&unit, &instance, |pass| {
            if pass.completion.is_some() {
                return Err("Explore is already finalizing".into());
            }
            let excluded = pass.coverage.unexplored_exclusions();
            if !units.iter().all(|unit| excluded.contains(unit)) {
                return Err("Exclusion is no longer current".into());
            }
            pass.coverage.require_review(units);
            Ok(())
        });
        match result {
            Ok(((), pass)) => {
                self.explore.pass = Some(pass.clone());
                let (response, _) = std::sync::mpsc::channel();
                let _ = messages.send(ui_events::ExploreCommitted {
                    pass: Arc::new(pass),
                    applied: true,
                    response,
                });
            }
            Err(error) => {
                let _ = messages.send(ui_events::ExploreStorageFailed(error.to_string()));
            }
        }
    }

    fn start_explore(&mut self, messages: &ApplicationMessageSender) {
        if self.explore.storage_error.is_some() || !self.cancel_explore_record(messages) {
            let _ = messages.send(ui_events::ExploreCaptured {
                result: Err(self
                    .explore
                    .storage_error
                    .clone()
                    .unwrap_or_else(|| "Explore could not save its state".into())),
            });
            return;
        }
        self.explore.prompt = None;
        self.explore.implementation = None;
        self.explore.pending = None;
        let result = self.capture_explore();
        if let Ok(comparison) = &result {
            self.explore.comparison = Some(comparison.clone());
            self.explore.agent = None;
            self.explore.pass = None;
            self.explore.restored = false;
            self.explore.historical = false;
            self.explore.access = uuid::Uuid::new_v4().to_string();
        }
        let _ = messages.send(ui_events::ExploreCaptured {
            result: result.map_err(|error| error.to_string()),
        });
    }

    fn capture_explore(&self) -> eyre::Result<Arc<Comparison>> {
        let review_repository::repository::PollResult::Complete(snapshot) =
            self.repository.poll()?
        else {
            eyre::bail!("Repository comparison is not ready; retry Start");
        };
        Ok(Arc::new(Comparison::prepare(&self.repository, &snapshot)?))
    }

    fn explore_turn(&mut self, request: TurnRequest, messages: &ApplicationMessageSender) {
        self.explore.prompt = None;
        self.explore.implementation = None;
        // Preserve the posted contribution even when its subsequent wakeup cannot be sent.
        let _ = self.bound_explore_agent();
        let persisted = self.persist_explore_request(&request);
        let pass = match persisted {
            Ok(pass) => pass,
            Err(error) => {
                let _ = messages.send(ui_events::ExplorePosted {
                    request,
                    result: Err(error.to_string()),
                });
                return;
            }
        };
        let pass = self.start_jev_if_enabled(pass, messages);
        self.explore.pass = Some(pass.clone());
        let _ = messages.send(ui_events::ExplorePosted {
            request: request.clone(),
            result: Ok(Arc::new(pass.clone())),
        });
        let (prepared, agent) = match self.prepare_explore(&request) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.explore.pending = Some((request.instance.clone(), request.request.clone()));
                self.explore_finished(
                    ui_events::ExploreFinished {
                        instance: request.instance,
                        request: request.request.clone(),
                        result: Err(error.to_string()),
                    },
                    &pass.turns[&request.request].attempt,
                    messages,
                );
                return;
            }
        };
        let observer = dispatch::DurableDispatch {
            began: std::sync::atomic::AtomicBool::default(),
            store: self.guide_store.clone(),
            unit: request.checkpoint.review_unit.clone(),
            instance: request.instance.clone(),
            id: review_explore::DispatchId::Interview {
                request: request.request.clone(),
                attempt: pass.turns[&request.request].attempt.clone(),
            },
            messages: messages.clone(),
        };
        let (receipt, cancellation) =
            self.prompts
                .send_observed(agent, prepared.prompt(), Some(Arc::new(observer)));
        self.explore.prompt = Some(cancellation);
        let commands = self.commands.clone();
        let attempt = pass.turns[&request.request].attempt.clone();
        std::thread::spawn(move || {
            if let Err(error) = receipt.wait() {
                let _ = commands.send(WorkerCommand::ExploreFinished {
                    event: Box::new(ui_events::ExploreFinished {
                        instance: request.instance,
                        request: request.request,
                        result: Err(error.to_string()),
                    }),
                    attempt,
                });
            }
        });
    }

    fn prepare_explore(
        &mut self,
        request: &TurnRequest,
    ) -> eyre::Result<(review_explore_runner::PreparedTurn, PinnedAgent)> {
        let comparison = self
            .explore
            .comparison
            .as_ref()
            .ok_or_else(|| eyre::eyre!("Start Explore first"))?;
        eyre::ensure!(
            comparison.checkpoint == request.checkpoint,
            "Request does not belong to this comparison"
        );
        let comparison = comparison.clone();
        let agent = self.bound_explore_agent()?;
        let prepared = review_explore_runner::PreparedTurn::prepare(
            request,
            &comparison,
            &self.explore.access,
        );
        self.explore.pending = Some((request.instance.clone(), request.request.clone()));
        self.explore.agent = Some(agent.clone());
        Ok((prepared, agent))
    }

    pub(super) fn explore_finished(
        &mut self,
        event: ui_events::ExploreFinished,
        attempt: &str,
        messages: &ApplicationMessageSender,
    ) {
        if self.explore.pending.as_ref() != Some(&(event.instance.clone(), event.request.clone())) {
            return;
        }
        if let Some(pass) = &self.explore.pass {
            let result = self.guide_store.update_explore(
                &pass.exploration.comparison.checkpoint.review_unit,
                &event.instance,
                |pass| {
                    if pass
                        .turns
                        .get(&event.request)
                        .is_none_or(|turn| turn.attempt != attempt)
                    {
                        return Ok(false);
                    }
                    if let Err(error) = &event.result {
                        return Ok(pass.exploration.failed(&event.request, error));
                    }
                    Ok(false)
                },
            );
            match result {
                Ok((true, pass)) => self.explore.pass = Some(pass),
                Ok((false, _)) => return,
                Err(error) => {
                    let _ = messages.send(ui_events::ExploreStorageFailed(error.to_string()));
                    return;
                }
            }
        }
        self.explore.prompt = None;
        let _ = messages.send(event);
    }

    pub(super) fn explore_mcp(
        &mut self,
        request: review_mcp::Request,
        messages: &ApplicationMessageSender,
    ) {
        if let Err(error) = self.authorize_explore(&request.access) {
            request.respond(Err(error.to_string()));
            return;
        }
        let update = match &request.operation {
            review_mcp::Operation::SubmitQuestion(update)
                if update.next.is_some() && update.conclusion.is_none() =>
            {
                (**update).clone()
            }
            review_mcp::Operation::SubmitConclusion(conclusion) => {
                (**conclusion).clone().into_update()
            }
            _ => {
                request.respond(Err(
                    "submit_question requires one question; use submit_conclusion to finish".into(),
                ));
                return;
            }
        };
        if self
            .explore
            .pass
            .as_ref()
            .is_none_or(|pass| pass.exploration.instance != update.instance)
        {
            request.respond(Err("Explore response belongs to another instance".into()));
            return;
        }
        let committed = self.guide_store.submit_explore(
            &update.checkpoint.review_unit,
            &update.instance,
            &update,
            jev::key().is_some(),
        );
        let (applied, pass, coverage) = match committed {
            Ok(result) => result,
            Err(error) => {
                request.respond(Err(error.to_string()));
                return;
            }
        };
        self.publish_explore_marks(&pass, messages);
        self.explore.pass = Some(pass.clone());
        if pass
            .completion
            .as_ref()
            .is_some_and(|completion| completion.completed)
        {
            self.explore.storage_error = None;
        }
        let (response, received) = std::sync::mpsc::channel();
        if messages
            .send(ui_events::ExploreCommitted {
                pass: Arc::new(pass),
                applied,
                response,
            })
            .is_err()
        {
            request.respond(Err(
                "The reviewer is closed; the accepted response was saved".into(),
            ));
            return;
        }
        std::thread::spawn(move || {
            let result = received
                .recv_timeout(std::time::Duration::from_secs(15))
                .map_err(|_| {
                    "The reviewer did not acknowledge Explore; retry the identical payload"
                        .to_owned()
                })
                .and_then(|result| result)
                .map(|applied| review_mcp::Response::Explore { applied, coverage });
            request.respond(result);
        });
    }

    fn start_jev_if_enabled(
        &mut self,
        pass: review_explore::ExplorePass,
        messages: &ApplicationMessageSender,
    ) -> review_explore::ExplorePass {
        let Some(key) = jev::key() else {
            return pass;
        };
        if pass.completion.is_some()
            || self.jev_running(&pass.exploration.instance)
            || !pass
                .coverage
                .needs_classification(jev::RUBRIC, pass.revision)
        {
            return pass;
        }
        let Some(comparison) = self.jev_comparison(&pass) else {
            return pass;
        };
        let all_candidates = jev::Candidate::prepare(&comparison);
        let total_windows = all_candidates.len();
        let candidates = all_candidates
            .into_iter()
            .filter(|candidate| !pass.coverage.classifications.contains_key(candidate.id()))
            .collect();
        let unit = pass.exploration.comparison.checkpoint.review_unit.clone();
        let instance = pass.exploration.instance.clone();
        let attempt = uuid::Uuid::new_v4().to_string();
        let Ok((started, pass)) = self.guide_store.update_explore(&unit, &instance, |pass| {
            if pass.completion.is_some()
                || !pass
                    .coverage
                    .needs_classification(jev::RUBRIC, pass.revision)
            {
                return Ok(false);
            }
            pass.coverage
                .restart_classification(jev::RUBRIC, attempt.clone());
            pass.coverage.jev_total_windows = total_windows;
            Ok(true)
        }) else {
            return pass;
        };
        if !started {
            return pass;
        }
        let prior_elapsed = pass.coverage.jev_elapsed_ms;
        let active = Arc::new(std::sync::atomic::AtomicBool::new(true));
        self.explore.jev_active = Some((instance.clone(), active.clone()));
        let store = self.guide_store.clone();
        let messages = messages.clone();
        std::thread::spawn(move || {
            let started = std::time::Instant::now();
            let finished = jev::classify(&key, candidates, |result| {
                match store.update_explore(&unit, &instance, |pass| {
                    if pass.coverage.classification_attempt.as_deref() != Some(&attempt)
                        || pass.completion.is_some()
                    {
                        return Err("obsolete classification attempt".into());
                    }
                    pass.coverage.jev_elapsed_ms = prior_elapsed.saturating_add(
                        u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                    );
                    Ok(pass.coverage.record_significance(result))
                }) {
                    Ok((true, pass)) => {
                        let (response, _) = std::sync::mpsc::channel();
                        messages
                            .send(ui_events::ExploreCommitted {
                                pass: Arc::new(pass),
                                applied: true,
                                response,
                            })
                            .is_ok()
                    }
                    Ok((false, _)) => true,
                    Err(_) => false,
                }
            });
            if let Ok((_, pass)) = store.update_explore(&unit, &instance, |pass| {
                if pass.coverage.classification_attempt.as_deref() != Some(&attempt) {
                    return Ok(false);
                }
                pass.coverage.jev_elapsed_ms = prior_elapsed.saturating_add(
                    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                );
                pass.coverage.classification_finished = finished;
                pass.coverage.revision += 1;
                Ok(true)
            }) {
                let (response, _) = std::sync::mpsc::channel();
                let _ = messages.send(ui_events::ExploreCommitted {
                    pass: Arc::new(pass),
                    applied: true,
                    response,
                });
            }
            active.store(false, std::sync::atomic::Ordering::Relaxed);
        });
        pass
    }

    fn jev_running(&self, instance: &str) -> bool {
        self.explore
            .jev_active
            .as_ref()
            .is_some_and(|(current, active)| {
                current == instance && active.load(std::sync::atomic::Ordering::Relaxed)
            })
    }

    /// Saved comparisons omit diff bytes; recapture only the same checkpoint.
    fn jev_comparison(&self, pass: &review_explore::ExplorePass) -> Option<Arc<Comparison>> {
        let comparison = match &self.explore.comparison {
            Some(comparison) if comparison.diffs.len() == comparison.files.len() => {
                comparison.clone()
            }
            _ => self.capture_explore().ok()?,
        };
        (comparison.checkpoint == pass.exploration.comparison.checkpoint).then_some(comparison)
    }

    fn publish_explore_marks(
        &self,
        pass: &review_explore::ExplorePass,
        messages: &ApplicationMessageSender,
    ) {
        let Some(completion) = &pass.completion else {
            return;
        };
        if !completion.completed {
            return;
        }
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        if snapshot.identity.review_unit() != &pass.exploration.comparison.checkpoint.review_unit {
            return;
        }
        for mark in &completion.marks {
            if let Some(file) = snapshot
                .files
                .iter()
                .find(|file| file.review_path().as_bytes() == mark.path)
            {
                let _ = messages.send(ui_events::ReviewStateSaved {
                    review_unit: snapshot.identity.review_unit().clone(),
                    path: file.review_path().display(),
                    result: self.tracker.status(snapshot, file).map_err(|_| ()),
                });
            }
        }
    }

    fn authorize_explore(&mut self, access: &str) -> eyre::Result<()> {
        eyre::ensure!(
            self.explore.storage_error.is_none()
                || self
                    .explore
                    .pass
                    .as_ref()
                    .and_then(|pass| pass.completion.as_ref())
                    .is_some_and(|completion| !completion.completed),
            "Explore storage is unavailable: {}",
            self.explore.storage_error.as_deref().unwrap_or_default()
        );
        eyre::ensure!(
            !access.is_empty() && self.explore.access == access && !self.explore.historical,
            "Obsolete Explore access; retry the interrupted turn from the reviewer for fresh access"
        );
        let pass = self
            .explore
            .pass
            .as_ref()
            .ok_or_else(|| eyre::eyre!("No Explore pass"))?;
        self.explore.pass = Some(
            self.guide_store
                .load_explore(
                    &pass.exploration.comparison.checkpoint.review_unit,
                    &pass.exploration.instance,
                )?
                .ok_or_else(|| {
                    eyre::eyre!(
                        "Saved Explore pass is missing; restore its state before continuing"
                    )
                })?,
        );
        let agent = self
            .bound_explore_agent()?
            .current(&self.client)
            .map_err(eyre::Report::msg)?
            .ok_or_else(|| eyre::eyre!("Waiting for the native agent conversation identity"))?;
        if let Some(binding) = review_explore::ConversationBinding::from_agent(&agent) {
            let pass = self.explore.pass.as_ref().expect("active pass");
            self.guide_store.update_explore(
                &pass.exploration.comparison.checkpoint.review_unit,
                &pass.exploration.instance,
                |pass| {
                    if let Some(previous) = &pass.binding {
                        if !previous.matches(&agent) {
                            return Err("Different native agent conversation".into());
                        }
                    } else {
                        pass.binding = Some(binding);
                    }
                    Ok(())
                },
            )?;
        }
        Ok(())
    }

    fn cancel_explore_record(&mut self, messages: &ApplicationMessageSender) -> bool {
        let Some(pass) = &self.explore.pass else {
            return true;
        };
        if self.explore.historical {
            return true;
        }
        if let Err(error) = self.guide_store.update_explore(
            &pass.exploration.comparison.checkpoint.review_unit,
            &pass.exploration.instance,
            |pass| {
                pass.exploration.cancel();
                Ok(())
            },
        ) {
            self.explore.storage_error = Some(error.to_string());
            let _ = messages.send(ui_events::ExploreStorageFailed(error.to_string()));
            return false;
        }
        true
    }
}
