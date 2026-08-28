use super::{
    Agent, AgentTarget, ChangedFile, Digest, FrozenFile, FrozenHunk, GuideMailbox,
    GuideRepositorySnapshot, GuideResponseVersion, GuideResponseWaitOutcome,
    GuideResponseWatchCancellation, GuideResult, GuideRunner, GuideScope, HerdrClient, Message,
    RepoPath, Repository, ReviewCheckpoint, ReviewStatus, ReviewStore, ReviewTracker, ReviewUnit,
    Sender, Sha256, Snapshot, WorkerCommand, parse_file_diff, thread,
};

#[derive(Debug, Default)]
pub(super) struct GuideRequestCoordinator {
    observed_response: Option<ObservedResponse>,
    response_wait: Option<ResponseWait>,
    generation_wait: Option<GenerationWait>,
    next_response_wait_token: u64,
}

#[derive(Debug)]
struct ObservedResponse {
    review_unit: ReviewUnit,
    version: GuideResponseVersion,
}

#[derive(Debug)]
struct ResponseWait {
    review_unit: ReviewUnit,
    token: u64,
    cancellation: GuideResponseWatchCancellation,
}

impl Drop for ResponseWait {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

#[derive(Debug)]
struct GenerationWait {
    review_unit: ReviewUnit,
    cancellation: GuideResponseWatchCancellation,
}

impl Drop for GenerationWait {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

#[derive(Debug)]
pub(super) struct FinishedGuide {
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
    pub(super) fn generate_review_guide(
        &mut self,
        context: &mut GuideOperationContext<'_>,
        messages: &Sender<Message>,
        scope: &GuideScope,
    ) {
        let Some(snapshot) = context.snapshot else {
            return;
        };
        let review_checkpoint = ReviewCheckpoint::new(
            snapshot.identity.review_unit().clone(),
            snapshot.identity.snapshot_id(),
        );
        Self::updating_status(messages, &review_checkpoint);
        let repository_snapshot =
            match Self::freeze_repository_snapshot(context, scope.clone(), &review_checkpoint) {
                Ok(repository_snapshot) => repository_snapshot,
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
        let mailbox_directory = match context
            .guide_store
            .guide_mailbox_directory(&review_checkpoint.review_unit)
        {
            Ok(directory) => directory,
            Err(error) => {
                Self::guide_error(
                    messages,
                    &review_checkpoint,
                    format!("guide request for agent {agent_name} failed: {error}"),
                );
                return;
            }
        };
        let prepared =
            match GuideRunner::<HerdrClient>::prepare(repository_snapshot, mailbox_directory) {
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
        let (response_watch, cancellation) = match prepared.watch_response() {
            Ok(watch) => watch,
            Err(error) => {
                Self::guide_error(
                    messages,
                    &review_checkpoint,
                    format!("guide request for agent {agent_name} failed: {error}"),
                );
                return;
            }
        };
        self.observed_response = None;
        self.generation_wait = Some(GenerationWait {
            review_unit: review_checkpoint.review_unit.clone(),
            cancellation,
        });
        Self::start_guide_thread(
            context,
            agent,
            prepared,
            response_watch,
            review_checkpoint,
            agent_name,
        );
    }

    fn start_guide_thread(
        context: &GuideOperationContext<'_>,
        agent: Agent,
        prepared: review_guide_runner::PreparedGuide,
        response_watch: review_guide_runner::GuideResponseWatch,
        review_checkpoint: ReviewCheckpoint,
        agent_name: String,
    ) {
        let commands = context.commands.clone();
        let client = context.client.clone();
        thread::spawn(move || {
            let runner = GuideRunner::new(&client);
            let result = runner
                .submit_prepared(&agent, &prepared)
                .and_then(|()| runner.finish_prepared_with_watch(&prepared, response_watch));
            let _ = commands.send(WorkerCommand::GuideFinished(FinishedGuide {
                review_checkpoint,
                agent_name,
                result,
            }));
        });
    }

    pub(super) fn import_completed_guide(
        &mut self,
        context: &GuideOperationContext<'_>,
        messages: &Sender<Message>,
        review_unit: &ReviewUnit,
    ) {
        self.cancel_waits_for_other_review_units(review_unit);
        let Ok(directory) = context.guide_store.guide_mailbox_directory(review_unit) else {
            return;
        };
        let Ok(mailbox) = GuideMailbox::open(directory) else {
            return;
        };
        match mailbox.response_version() {
            Ok(Some(version)) if !self.has_observed_response(review_unit, version) => {}
            Ok(Some(version)) => {
                self.start_response_wait(context, review_unit, &mailbox, Some(version));
                return;
            }
            Ok(None) if mailbox.is_prepared() => {
                self.start_response_wait(context, review_unit, &mailbox, None);
                return;
            }
            Ok(None) | Err(_) => return,
        }
        match mailbox.load_completed_guide() {
            Ok(result) => {
                let response_version = result.response_version;
                self.observe_response(review_unit, response_version);
                Self::accept_guide(context, messages, result);
                self.start_response_wait(context, review_unit, &mailbox, Some(response_version));
            }
            Err(error) => {
                let Some(snapshot) = context.snapshot else {
                    return;
                };
                Self::guide_error(
                    messages,
                    &ReviewCheckpoint::new(review_unit.clone(), snapshot.identity.snapshot_id()),
                    format!("could not import the completed review guide: {error}"),
                );
            }
        }
    }

    pub(super) fn response_ready(
        &mut self,
        context: &GuideOperationContext<'_>,
        messages: &Sender<Message>,
        review_unit: &ReviewUnit,
        wait_token: u64,
    ) {
        if !self
            .response_wait
            .as_ref()
            .is_some_and(|wait| &wait.review_unit == review_unit && wait.token == wait_token)
        {
            return;
        }
        self.response_wait = None;
        self.import_completed_guide(context, messages, review_unit);
    }

    pub(super) fn finish_review_guide(
        &mut self,
        context: &GuideOperationContext<'_>,
        messages: &Sender<Message>,
        finished: FinishedGuide,
    ) {
        if !context.snapshot.is_some_and(|snapshot| {
            snapshot.identity.review_unit() == &finished.review_checkpoint.review_unit
        }) {
            return;
        }
        match finished.result {
            Ok(result) => {
                let response_version = result.response_version;
                let review_unit = result.guide.review_checkpoint.review_unit.clone();
                let mailbox = context
                    .guide_store
                    .guide_mailbox_directory(&review_unit)
                    .ok()
                    .and_then(|directory| GuideMailbox::open(directory).ok());
                let current_response = mailbox
                    .as_ref()
                    .and_then(|mailbox| mailbox.response_version().ok())
                    .flatten();
                if current_response != Some(response_version) {
                    self.import_completed_guide(context, messages, &review_unit);
                    return;
                }
                if self.has_observed_response(&review_unit, response_version) {
                    if let Some(mailbox) = mailbox {
                        self.start_response_wait(
                            context,
                            &review_unit,
                            &mailbox,
                            Some(response_version),
                        );
                    }
                    return;
                }
                self.observe_response(&review_unit, response_version);
                Self::accept_guide(context, messages, result);
                if let Some(mailbox) = mailbox {
                    self.start_response_wait(
                        context,
                        &review_unit,
                        &mailbox,
                        Some(response_version),
                    );
                }
            }
            Err(review_guide_runner::Error::ResponseWaitCancelled) => {}
            Err(error) => Self::guide_error(
                messages,
                &finished.review_checkpoint,
                format!(
                    "guide request for agent {} failed: {error}",
                    finished.agent_name
                ),
            ),
        }
    }

    fn start_response_wait(
        &mut self,
        context: &GuideOperationContext<'_>,
        review_unit: &ReviewUnit,
        mailbox: &GuideMailbox,
        previous: Option<GuideResponseVersion>,
    ) {
        if self
            .response_wait
            .as_ref()
            .is_some_and(|wait| &wait.review_unit == review_unit)
        {
            return;
        }
        let Ok((watch, cancellation)) = mailbox.watch_response_after(previous) else {
            return;
        };
        self.next_response_wait_token = self.next_response_wait_token.wrapping_add(1);
        let wait_token = self.next_response_wait_token;
        self.response_wait = Some(ResponseWait {
            review_unit: review_unit.clone(),
            token: wait_token,
            cancellation,
        });
        let commands = context.commands.clone();
        let review_unit = review_unit.clone();
        thread::spawn(move || {
            if matches!(watch.wait(), Ok(GuideResponseWaitOutcome::ResponseChanged)) {
                let _ = commands.send(WorkerCommand::ImportReviewGuide {
                    review_unit,
                    wait_token,
                });
            }
        });
    }

    fn cancel_waits_for_other_review_units(&mut self, review_unit: &ReviewUnit) {
        if self
            .response_wait
            .as_ref()
            .is_some_and(|wait| &wait.review_unit != review_unit)
        {
            self.response_wait = None;
        }
        if self
            .generation_wait
            .as_ref()
            .is_some_and(|wait| &wait.review_unit != review_unit)
        {
            self.generation_wait = None;
        }
    }

    fn has_observed_response(
        &self,
        review_unit: &ReviewUnit,
        version: GuideResponseVersion,
    ) -> bool {
        self.observed_response.as_ref().is_some_and(|observed| {
            &observed.review_unit == review_unit && observed.version == version
        })
    }

    fn observe_response(&mut self, review_unit: &ReviewUnit, version: GuideResponseVersion) {
        self.observed_response = Some(ObservedResponse {
            review_unit: review_unit.clone(),
            version,
        });
    }

    fn accept_guide(
        context: &GuideOperationContext<'_>,
        messages: &Sender<Message>,
        result: GuideResult,
    ) {
        if let Err(error) = context.guide_store.save_guide(&result.guide) {
            Self::guide_error(
                messages,
                &result.guide.review_checkpoint,
                format!("could not store the accepted review guide: {error}"),
            );
            return;
        }
        Self::show_guide_for_current_checkpoint(context, messages, &result.guide);
        let message = (result.rejected_items > 0).then(|| {
            format!(
                "invalid review guide items ignored: {}",
                result.rejected_items
            )
        });
        let _ = messages.send(Message::ReviewGuideStatus {
            review_checkpoint: result.guide.review_checkpoint,
            generating: false,
            message,
        });
    }

    fn show_guide_for_current_checkpoint(
        context: &GuideOperationContext<'_>,
        messages: &Sender<Message>,
        guide: &review_guide::GuideSnapshot,
    ) {
        let Some(snapshot) = context.snapshot else {
            return;
        };
        if &guide.review_checkpoint.review_unit != snapshot.identity.review_unit() {
            return;
        }
        let review_checkpoint = ReviewCheckpoint::new(
            snapshot.identity.review_unit().clone(),
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

    fn freeze_repository_snapshot(
        context: &GuideOperationContext<'_>,
        scope: GuideScope,
        review_checkpoint: &ReviewCheckpoint,
    ) -> eyre::Result<GuideRepositorySnapshot> {
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
        Ok(GuideRepositorySnapshot {
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

    fn updating_status(messages: &Sender<Message>, review_checkpoint: &ReviewCheckpoint) {
        let _ = messages.send(Message::ReviewGuideStatus {
            review_checkpoint: review_checkpoint.clone(),
            generating: true,
            message: None,
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

#[cfg(test)]
#[path = "guide.tests.rs"]
mod tests;
