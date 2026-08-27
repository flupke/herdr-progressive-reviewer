use super::{
    Agent, AgentStatus, AgentTarget, Arc, ChangedFile, Digest, FrozenFile, FrozenHunk,
    GuideCancellation, GuideRequest, GuideRequestId, GuideRequestRecord, GuideRequestState,
    GuideResult, GuideRunner, GuideScope, HerdrClient, HerdrEvent, Message, PreparedGuide,
    RepoPath, Repository, ReviewCheckpoint, ReviewStatus, ReviewStore, ReviewTracker, Sender,
    Sha256, Snapshot, WorkerCommand, parse_file_diff, thread,
};

#[derive(Debug, Default)]
pub(super) struct GuideRequestCoordinator {
    active: Option<ActiveGuide>,
    reconciled_review_unit: Option<String>,
}

#[derive(Debug)]
struct ActiveGuide {
    request_id: GuideRequestId,
    agent: Agent,
    cancellation: Arc<GuideCancellation>,
    record: GuideRequestRecord,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GuideAgentEvent {
    Ignore,
    Block,
}

#[derive(Debug)]
pub(super) struct FinishedGuide {
    request_id: GuideRequestId,
    review_checkpoint: ReviewCheckpoint,
    agent_name: String,
    result: Result<GuideResult, review_guide_runner::Error>,
}

pub(super) struct GuideOperationContext<'a> {
    repository: &'a Repository,
    tracker: &'a ReviewTracker,
    guide_store: &'a ReviewStore,
    client: &'a HerdrClient,
    target: &'a mut AgentTarget,
    snapshot: Option<&'a Snapshot>,
    commands: &'a Sender<WorkerCommand>,
}

impl<'a> GuideOperationContext<'a> {
    pub(super) fn new(
        repository: &'a Repository,
        tracker: &'a ReviewTracker,
        guide_store: &'a ReviewStore,
        client: &'a HerdrClient,
        target: &'a mut AgentTarget,
        snapshot: Option<&'a Snapshot>,
        commands: &'a Sender<WorkerCommand>,
    ) -> Self {
        Self {
            repository,
            tracker,
            guide_store,
            client,
            target,
            snapshot,
            commands,
        }
    }
}

impl GuideRequestCoordinator {
    pub(super) fn mark_submitted(
        &mut self,
        context: &GuideOperationContext<'_>,
        request_id: &GuideRequestId,
    ) {
        if let Some(active) = &mut self.active
            && &active.request_id == request_id
        {
            active.record.state = GuideRequestState::Submitted;
            let _ = context.guide_store.save_guide_request(&mut active.record);
        }
    }

    pub(super) fn stop(&mut self, context: &GuideOperationContext<'_>) {
        if let Some(active) = &mut self.active {
            active.cancellation.cancel();
            active.record.state = GuideRequestState::Incomplete;
            active.record.error = None;
            let _ = context.guide_store.save_guide_request(&mut active.record);
        }
    }

    pub(super) fn reconcile_incomplete_guides(
        &mut self,
        context: &mut GuideOperationContext<'_>,
        messages: &Sender<Message>,
        review_unit: &str,
    ) {
        if self.reconciled_review_unit.as_deref() == Some(review_unit) {
            return;
        }
        let mut records = match context.guide_store.load_guide_requests(review_unit) {
            Ok(records) => records,
            Err(error) => {
                if let Some(snapshot) = context.snapshot {
                    Self::guide_error(
                        messages,
                        &ReviewCheckpoint::new(review_unit, snapshot.identity.snapshot_id()),
                        format!("could not load incomplete review guide requests: {error}"),
                    );
                }
                return;
            }
        };
        self.reconciled_review_unit = Some(review_unit.to_owned());
        records.reverse();
        let mut resumed = false;
        for mut record in records.into_iter().filter(|record| {
            matches!(
                record.state,
                GuideRequestState::Preparing
                    | GuideRequestState::Submitted
                    | GuideRequestState::Incomplete
            )
        }) {
            if resumed
                || self.active.is_some()
                || !self.resume_request(context, messages, &mut record)
            {
                if let Some(path) = record.transport_directory.take() {
                    let _ = review_guide_runner::abandon_transport(path);
                }
                record.state = GuideRequestState::Failed;
                record.error = Some("incomplete request could not be resumed".to_owned());
                let _ = context.guide_store.save_guide_request(&mut record);
            } else {
                resumed = true;
            }
        }
    }

    fn resume_request(
        &mut self,
        context: &mut GuideOperationContext<'_>,
        messages: &Sender<Message>,
        record: &mut GuideRequestRecord,
    ) -> bool {
        let Some(path) = record.transport_directory.clone() else {
            return false;
        };
        let Ok(prepared) = PreparedGuide::resume(record.request_id.clone(), path) else {
            return false;
        };
        Self::updating_status(messages, &record.review_checkpoint, None);
        self.start_guide_thread(
            context,
            record.agent.clone(),
            prepared,
            record.clone(),
            true,
        );
        true
    }

    pub(super) fn generate_review_guide(
        &mut self,
        context: &mut GuideOperationContext<'_>,
        messages: &Sender<Message>,
        scope: &GuideScope,
    ) {
        if self.active.is_some() {
            let Some(snapshot) = context.snapshot else {
                return;
            };
            Self::updating_status(
                messages,
                &ReviewCheckpoint::new(
                    snapshot.identity.review_id(),
                    snapshot.identity.snapshot_id(),
                ),
                Some("A guide update is already in progress".to_owned()),
            );
            return;
        }
        let Some(snapshot) = context.snapshot else {
            return;
        };
        let review_unit = snapshot.identity.review_id().to_owned();
        let checkpoint = snapshot.identity.snapshot_id().to_owned();
        let review_checkpoint = ReviewCheckpoint::new(&review_unit, &checkpoint);
        let _ = messages.send(Message::ReviewGuideStatus {
            review_checkpoint: review_checkpoint.clone(),
            generating: true,
            message: None,
        });
        let request = match Self::freeze_guide_request(context, scope.clone(), &review_checkpoint) {
            Ok(request) => request,
            Err(error) => {
                Self::guide_error(messages, &review_checkpoint, error.to_string());
                return;
            }
        };
        let agent = match context.target.resolve(context.client) {
            Ok(Some(agent)) => agent,
            Ok(None) => {
                Self::guide_error(
                    messages,
                    &review_checkpoint,
                    "no active implementation agent is available".to_owned(),
                );
                return;
            }
            Err(error) => {
                Self::guide_error(messages, &review_checkpoint, error.to_string());
                return;
            }
        };
        let agent_name = display_agent_name(&agent);
        Self::updating_status(messages, &review_checkpoint, None);
        let prepared = match GuideRunner::<HerdrClient>::prepare(request) {
            Ok(prepared) => prepared,
            Err(error) => {
                Self::guide_error(
                    messages,
                    &review_checkpoint,
                    format!("guide request for agent {agent_name} failed: {error}"),
                );
                return;
            }
        };
        let request_id = prepared.request_id().to_owned();
        let mut record =
            Self::preparing_record(review_checkpoint, request_id, scope, &agent, &prepared);
        if let Err(error) = context.guide_store.save_guide_request(&mut record) {
            let _ =
                review_guide_runner::abandon_transport(prepared.transport_directory().to_owned());
            Self::guide_error(
                messages,
                &record.review_checkpoint,
                format!("guide request for agent {agent_name} failed: {error}"),
            );
            return;
        }
        self.start_guide_thread(context, agent, prepared, record, false);
    }

    fn preparing_record(
        review_checkpoint: ReviewCheckpoint,
        request_id: GuideRequestId,
        scope: &GuideScope,
        agent: &Agent,
        prepared: &PreparedGuide,
    ) -> GuideRequestRecord {
        GuideRequestRecord {
            schema_version: 1,
            review_checkpoint,
            request_id,
            scope: scope.clone(),
            agent: agent.clone(),
            state: GuideRequestState::Preparing,
            created_at: String::new(),
            updated_at: String::new(),
            transport_directory: Some(prepared.transport_directory().to_owned()),
            error: None,
        }
    }

    fn start_guide_thread(
        &mut self,
        context: &mut GuideOperationContext<'_>,
        agent: Agent,
        prepared: PreparedGuide,
        record: GuideRequestRecord,
        resume: bool,
    ) {
        let request_id = record.request_id.clone();
        let review_checkpoint = record.review_checkpoint.clone();
        let cancellation = Arc::new(GuideCancellation::default());
        self.active = Some(ActiveGuide {
            request_id: request_id.clone(),
            agent: agent.clone(),
            cancellation: Arc::clone(&cancellation),
            record,
        });
        let commands = context.commands.clone();
        let client = context.client.clone();
        let agent_name = display_agent_name(&agent);
        thread::spawn(move || {
            let runner = GuideRunner::new(&client);
            let result = if resume {
                runner.finish_prepared(&agent, prepared, cancellation.as_ref())
            } else {
                match runner.submit_prepared(&agent, &prepared, cancellation.as_ref()) {
                    Ok(()) => {
                        let _ = commands.send(WorkerCommand::GuideSubmitted {
                            request_id: request_id.clone(),
                        });
                        runner.finish_prepared(&agent, prepared, cancellation.as_ref())
                    }
                    Err(error) => {
                        let _ = review_guide_runner::abandon_transport(
                            prepared.transport_directory().to_owned(),
                        );
                        Err(error)
                    }
                }
            };
            let _ = commands.send(WorkerCommand::GuideFinished(FinishedGuide {
                request_id,
                review_checkpoint,
                agent_name,
                result,
            }));
        });
    }

    pub(super) fn finish_review_guide(
        &mut self,
        context: &mut GuideOperationContext<'_>,
        messages: &Sender<Message>,
        finished: FinishedGuide,
    ) {
        let FinishedGuide {
            request_id,
            review_checkpoint,
            agent_name,
            result,
        } = finished;
        if self.active.as_ref().map(|guide| &guide.request_id) != Some(&request_id) {
            return;
        }
        let mut active = self.active.take().expect("the active request ID matched");
        match result {
            Ok(result) => {
                if let Err(error) = context.guide_store.save_guide(&result.guide) {
                    let _ = Self::complete_request_record(
                        context,
                        &mut active.record,
                        GuideRequestState::Failed,
                        Some("could not store the accepted review guide".to_owned()),
                    );
                    Self::guide_error(
                        messages,
                        &review_checkpoint,
                        format!("guide request for agent {agent_name} failed: {error}"),
                    );
                    return;
                }
                Self::show_guide_for_current_checkpoint(context, messages, &result.guide);
                if let Err(error) = Self::complete_request_record(
                    context,
                    &mut active.record,
                    GuideRequestState::Completed,
                    None,
                ) {
                    Self::guide_error(
                        messages,
                        &result.guide.review_checkpoint,
                        format!(
                            "guide request for agent {agent_name} succeeded, but its completion state could not be stored: {error}"
                        ),
                    );
                    return;
                }
                if result.rejected_items > 0 {
                    let _ = messages.send(Message::ReviewGuideStatus {
                        review_checkpoint: result.guide.review_checkpoint,
                        generating: false,
                        message: Some(format!(
                            "invalid review guide items ignored: {}",
                            result.rejected_items
                        )),
                    });
                }
            }
            Err(error) => {
                let _ = Self::complete_request_record(
                    context,
                    &mut active.record,
                    GuideRequestState::Failed,
                    Some(error.to_string()),
                );
                Self::guide_error(
                    messages,
                    &review_checkpoint,
                    format!("guide request for agent {agent_name} failed: {error}"),
                );
            }
        }
    }

    fn show_guide_for_current_checkpoint(
        context: &GuideOperationContext<'_>,
        messages: &Sender<Message>,
        guide: &review_guide::GuideSnapshot,
    ) {
        let Some(snapshot) = context.snapshot else {
            return;
        };
        if guide.review_checkpoint.review_unit != snapshot.identity.review_id() {
            return;
        }
        let review_checkpoint = ReviewCheckpoint::new(
            snapshot.identity.review_id(),
            snapshot.identity.snapshot_id(),
        );
        let items = if guide.review_checkpoint == review_checkpoint {
            guide.items.clone()
        } else {
            let current_files = snapshot
                .files
                .iter()
                .filter(|file| {
                    context
                        .tracker
                        .status(snapshot, file)
                        .is_ok_and(|state| state.status != ReviewStatus::Reviewed)
                })
                .filter_map(|file| {
                    Self::frozen_file(context, snapshot, file)
                        .ok()
                        .map(|value| value.0)
                })
                .collect::<Vec<_>>();
            review_guide::map_anchored_items(&guide.anchored_items, &current_files)
        };
        let _ = messages.send(Message::ReviewGuideLoaded {
            review_checkpoint,
            items,
        });
    }

    fn complete_request_record(
        context: &GuideOperationContext<'_>,
        record: &mut GuideRequestRecord,
        state: GuideRequestState,
        error: Option<String>,
    ) -> Result<(), review_store::Error> {
        record.state = state;
        record.transport_directory = None;
        record.error = error.map(|error| error.chars().take(512).collect());
        context.guide_store.save_guide_request(record)
    }

    pub(super) fn observe_guide_event(&mut self, event: &HerdrEvent) {
        let Some(active) = &self.active else {
            return;
        };
        match classify_guide_agent_event(&active.agent, event) {
            GuideAgentEvent::Ignore => {}
            GuideAgentEvent::Block => active.cancellation.block(),
        }
    }

    fn freeze_guide_request(
        context: &GuideOperationContext<'_>,
        scope: GuideScope,
        review_checkpoint: &ReviewCheckpoint,
    ) -> eyre::Result<GuideRequest> {
        use std::fmt::Write as _;

        let snapshot = context
            .snapshot
            .as_ref()
            .ok_or_else(|| eyre::eyre!("the repository snapshot is not ready"))?;
        let selected = snapshot.files.iter().filter(|file| match &scope {
            GuideScope::File { path } => file.review_path().display() == *path,
            GuideScope::All => context
                .tracker
                .status(snapshot, file)
                .is_ok_and(|state| state.status != ReviewStatus::Reviewed),
        });
        let mut frozen_diff = String::new();
        let mut files = Vec::new();
        for file in selected {
            let (frozen_file, unified) = Self::frozen_file(context, snapshot, file)?;
            let path = frozen_file.path.clone();
            writeln!(frozen_diff, "\n=== FILE {path} ===")?;
            let mut hunk = 0;
            for line in unified.split_inclusive(|byte| *byte == b'\n') {
                if line.starts_with(b"@@") {
                    hunk += 1;
                    writeln!(frozen_diff, "=== HUNK {hunk} ===")?;
                }
                frozen_diff.push_str(&String::from_utf8_lossy(line));
            }
            if frozen_file.hunk_count == 0 {
                writeln!(frozen_diff, "=== NO TEXT HUNKS ===")?;
            }
            files.push(frozen_file);
        }
        if files.is_empty() {
            eyre::bail!("the requested guide scope has no visible unreviewed file");
        }
        let (previous_items, previous_anchored_items) = match context
            .guide_store
            .load_guide(&review_checkpoint.review_unit)?
        {
            Some(guide) if guide.review_checkpoint.checkpoint == review_checkpoint.checkpoint => {
                (guide.items, guide.anchored_items)
            }
            Some(guide) => {
                let current_files = snapshot
                    .files
                    .iter()
                    .filter(|file| {
                        context
                            .tracker
                            .status(snapshot, file)
                            .is_ok_and(|state| state.status != ReviewStatus::Reviewed)
                    })
                    .filter_map(|file| {
                        Self::frozen_file(context, snapshot, file)
                            .ok()
                            .map(|value| value.0)
                    })
                    .collect::<Vec<_>>();
                let mapped_items =
                    review_guide::map_anchored_items(&guide.anchored_items, &current_files);
                (mapped_items, guide.anchored_items)
            }
            None => (Vec::new(), Vec::new()),
        };
        Ok(GuideRequest {
            repository_root: context.repository.root().to_owned(),
            review_checkpoint: review_checkpoint.clone(),
            scope,
            frozen_diff,
            files,
            previous_items,
            previous_anchored_items,
        })
    }

    pub(super) fn frozen_file(
        context: &GuideOperationContext<'_>,
        snapshot: &Snapshot,
        file: &ChangedFile,
    ) -> eyre::Result<(FrozenFile, Vec<u8>)> {
        let diff = context.tracker.diff(snapshot, file)?;
        let diff_hash = format!("{:x}", Sha256::digest(&diff.unified));
        let rows = parse_file_diff(&diff.unified, file);
        let hunks = rows
            .iter()
            .filter_map(|row| match row {
                review_repository::diff::DiffRow::Hunk {
                    old_start,
                    old_count,
                    new_start,
                    new_count,
                } => Some(FrozenHunk {
                    old: (*old_count > 0).then(|| {
                        old_start.saturating_sub(1)
                            ..old_start.saturating_sub(1).saturating_add(*old_count)
                    }),
                    new: (*new_count > 0).then(|| {
                        new_start.saturating_sub(1)
                            ..new_start.saturating_sub(1).saturating_add(*new_count)
                    }),
                }),
                _ => None,
            })
            .collect::<Vec<_>>();
        Ok((
            FrozenFile {
                path: file.review_path().display(),
                hunk_count: hunks.len(),
                old_path: file.old_path.as_ref().map(RepoPath::display),
                new_path: file.new_path.as_ref().map(RepoPath::display),
                old_content: diff.old_content,
                new_content: diff.new_content,
                hunks,
                diff_hash,
            },
            diff.unified,
        ))
    }

    fn guide_error(
        messages: &Sender<Message>,
        review_checkpoint: &ReviewCheckpoint,
        message: String,
    ) {
        let _ = messages.send(Message::ReviewGuideStatus {
            review_checkpoint: review_checkpoint.clone(),
            generating: false,
            message: Some(message),
        });
    }

    fn updating_status(
        messages: &Sender<Message>,
        review_checkpoint: &ReviewCheckpoint,
        message: Option<String>,
    ) {
        let _ = messages.send(Message::ReviewGuideStatus {
            review_checkpoint: review_checkpoint.clone(),
            generating: true,
            message,
        });
    }
}

fn display_agent_name(agent: &Agent) -> String {
    agent
        .name
        .clone()
        .or_else(|| agent.display_agent.clone())
        .unwrap_or_else(|| agent.pane_id.0.clone())
}

fn classify_guide_agent_event(active_agent: &Agent, event: &HerdrEvent) -> GuideAgentEvent {
    match event {
        HerdrEvent::AgentStatusChanged {
            pane_id,
            workspace_id,
            status,
            ..
        } => {
            if pane_id == &active_agent.pane_id
                && workspace_id == &active_agent.workspace_id
                && *status == AgentStatus::Blocked
            {
                return GuideAgentEvent::Block;
            }
        }
        HerdrEvent::PaneFocused(_) | HerdrEvent::AgentDetected { .. } => {}
    }
    GuideAgentEvent::Ignore
}

#[cfg(test)]
#[path = "guide.tests.rs"]
mod tests;
