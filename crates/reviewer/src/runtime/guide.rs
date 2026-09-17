use super::{
    Agent, AgentTarget, ApplicationMessageSender, ChangedFile, Digest, FrozenFile, FrozenHunk,
    GuideMailbox, GuideRepositorySnapshot, GuideResponseVersion, GuideResponseWaitOutcome,
    GuideResponseWatchCancellation, GuideResult, GuideScope, HerdrClient, PreparedGuide, RepoPath,
    Repository, ReviewCheckpoint, ReviewStatus, ReviewStore, ReviewTracker, ReviewUnit, Sender,
    Sha256, Snapshot, WorkerCommand, parse_file_diff, thread,
};
use ui_events::{ReviewGuideChanged, ReviewGuideStatusChanged};

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
    _prompt: review_thread_service::PromptCancellation,
}

struct PreparedGeneration {
    agent: Agent,
    agent_name: String,
    prepared: review_guide_runner::PreparedGuide,
    response_watch: review_guide_runner::GuideResponseWatch,
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
    pub(super) repository: &'a Repository,
    pub(super) tracker: &'a ReviewTracker,
    pub(super) guide_store: &'a ReviewStore,
    pub(super) client: &'a HerdrClient,
    pub(super) target: &'a mut AgentTarget,
    pub(super) snapshot: Option<&'a Snapshot>,
    pub(super) prompts: &'a review_thread_service::PromptSender,
    pub(super) commands: &'a Sender<WorkerCommand>,
}

impl GuideRequestCoordinator {
    pub(super) fn generate_review_guide(
        &mut self,
        context: &mut GuideOperationContext<'_>,
        messages: &ApplicationMessageSender,
        scope: &GuideScope,
    ) {
        let Some(snapshot) = context.snapshot else {
            return;
        };
        // Cancel unsent work before preparing a replacement's shared inputs.
        self.generation_wait = None;
        let review_checkpoint = ReviewCheckpoint::new(
            snapshot.identity.review_unit().clone(),
            snapshot.identity.snapshot_id(),
        );
        Self::updating_status(messages, &review_checkpoint);
        let generation = match Self::prepare_generation(context, scope, &review_checkpoint) {
            Ok(generation) => generation,
            Err(error) => {
                Self::guide_error(messages, &review_checkpoint, error);
                return;
            }
        };
        self.observed_response = None;
        let prompt = Self::start_guide_thread(
            context,
            generation.agent,
            generation.prepared,
            generation.response_watch,
            review_checkpoint.clone(),
            generation.agent_name,
        );
        self.generation_wait = Some(GenerationWait {
            review_unit: review_checkpoint.review_unit,
            cancellation: generation.cancellation,
            _prompt: prompt,
        });
    }

    fn prepare_generation(
        context: &mut GuideOperationContext<'_>,
        scope: &GuideScope,
        review_checkpoint: &ReviewCheckpoint,
    ) -> Result<PreparedGeneration, String> {
        let repository_snapshot =
            Self::freeze_repository_snapshot(context, scope.clone(), review_checkpoint)
                .map_err(|error| error.to_string())?;
        let agent = context
            .target
            .resolve(context.client)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "no active implementation agent is available".to_owned())?;
        let agent_name = display_agent_name(&agent);
        let mailbox_directory = context
            .guide_store
            .guide_mailbox_directory(&review_checkpoint.review_unit)
            .map_err(|error| guide_request_error(&agent_name, &error))?;
        let prepared = PreparedGuide::prepare(repository_snapshot, mailbox_directory)
            .map_err(|error| guide_request_error(&agent_name, &error))?;
        let (response_watch, cancellation) = prepared
            .watch_response()
            .map_err(|error| guide_request_error(&agent_name, &error))?;
        Ok(PreparedGeneration {
            agent,
            agent_name,
            prepared,
            response_watch,
            cancellation,
        })
    }

    fn start_guide_thread(
        context: &GuideOperationContext<'_>,
        agent: Agent,
        prepared: review_guide_runner::PreparedGuide,
        response_watch: review_guide_runner::GuideResponseWatch,
        review_checkpoint: ReviewCheckpoint,
        agent_name: String,
    ) -> review_thread_service::PromptCancellation {
        let commands = context.commands.clone();
        let (receipt, cancellation) = context.prompts.send(
            review_thread_service::PinnedAgent::new(agent),
            prepared.prompt(),
        );
        thread::spawn(move || {
            let result = receipt
                .wait()
                .map_err(|error| match error {
                    review_thread_service::PromptError::Cancelled => {
                        review_guide_runner::Error::ResponseWaitCancelled
                    }
                    review_thread_service::PromptError::Delivery(message) => {
                        review_guide_runner::Error::Operation {
                            operation: "submit review guide prompt",
                            message,
                        }
                    }
                })
                .and_then(|()| prepared.finish_with_watch(response_watch));
            let _ = commands.send(WorkerCommand::GuideFinished(Box::new(FinishedGuide {
                review_checkpoint,
                agent_name,
                result,
            })));
        });
        cancellation
    }

    pub(super) fn import_completed_guide(
        &mut self,
        context: &GuideOperationContext<'_>,
        messages: &ApplicationMessageSender,
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
        messages: &ApplicationMessageSender,
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
        messages: &ApplicationMessageSender,
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
        messages: &ApplicationMessageSender,
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
        let _ = messages.send(ReviewGuideStatusChanged {
            review_checkpoint: result.guide.review_checkpoint,
            generating: false,
            message,
        });
    }

    fn show_guide_for_current_checkpoint(
        context: &GuideOperationContext<'_>,
        messages: &ApplicationMessageSender,
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
            let Ok(unreviewed_files) = Self::unreviewed_files(context, snapshot) else {
                return;
            };
            let current_files = Self::frozen_files(context, snapshot, &unreviewed_files);
            review_guide::map_anchored_items(&guide.anchored_items, &current_files)
        };
        let _ = messages.send(ReviewGuideChanged {
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
        let (selected_files, unreviewed_files) = Self::files_for_scope(context, snapshot, &scope)?;
        let mut frozen_diff = String::new();
        let mut files = Vec::new();
        for file in selected_files {
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
        let (previous_items, previous_anchored_items) =
            Self::previous_guide_items(context, snapshot, review_checkpoint, unreviewed_files)?;
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

    fn files_for_scope<'a>(
        context: &GuideOperationContext<'_>,
        snapshot: &'a Snapshot,
        scope: &GuideScope,
    ) -> eyre::Result<(Vec<&'a ChangedFile>, Option<Vec<&'a ChangedFile>>)> {
        match scope {
            GuideScope::File { path } => Ok((
                snapshot
                    .files
                    .iter()
                    .filter(|file| file.review_path().display() == *path)
                    .collect(),
                None,
            )),
            GuideScope::All => {
                let unreviewed_files = Self::unreviewed_files(context, snapshot)?;
                Ok((unreviewed_files.clone(), Some(unreviewed_files)))
            }
        }
    }

    fn previous_guide_items(
        context: &GuideOperationContext<'_>,
        snapshot: &Snapshot,
        review_checkpoint: &ReviewCheckpoint,
        unreviewed_files: Option<Vec<&ChangedFile>>,
    ) -> eyre::Result<(
        Vec<review_guide::GuideItem>,
        Vec<review_guide::AnchoredGuideItem>,
    )> {
        let Some(guide) = context
            .guide_store
            .load_guide(&review_checkpoint.review_unit)?
        else {
            return Ok((Vec::new(), Vec::new()));
        };
        if guide.review_checkpoint.checkpoint == review_checkpoint.checkpoint {
            return Ok((guide.items, guide.anchored_items));
        }

        let unreviewed_files = match unreviewed_files {
            Some(files) => files,
            None => Self::unreviewed_files(context, snapshot)?,
        };
        let current_files = Self::frozen_files(context, snapshot, &unreviewed_files);
        let mapped_items = review_guide::map_anchored_items(&guide.anchored_items, &current_files);
        Ok((mapped_items, guide.anchored_items))
    }

    fn unreviewed_files<'a>(
        context: &GuideOperationContext<'_>,
        snapshot: &'a Snapshot,
    ) -> eyre::Result<Vec<&'a ChangedFile>> {
        let states = context.tracker.statuses(snapshot)?;
        Ok(snapshot
            .files
            .iter()
            .zip(states)
            .filter_map(|(file, state)| (state.status != ReviewStatus::Reviewed).then_some(file))
            .collect())
    }

    pub(super) fn frozen_files(
        context: &GuideOperationContext<'_>,
        snapshot: &Snapshot,
        files: &[&ChangedFile],
    ) -> Vec<FrozenFile> {
        if files.is_empty() {
            return Vec::new();
        }
        // Each file needs several repository commands. Bound concurrent processes
        // while preserving repository order for guide overlap detection.
        thread::scope(|scope| {
            let readers = files
                .chunks(files.len().div_ceil(4))
                .map(|chunk| {
                    scope.spawn(move || {
                        chunk
                            .iter()
                            .filter_map(|file| {
                                Self::frozen_file(context, snapshot, file)
                                    .ok()
                                    .map(|value| value.0)
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect::<Vec<_>>();
            readers
                .into_iter()
                .flat_map(|reader| reader.join().expect("guide file reader did not panic"))
                .collect()
        })
    }

    fn frozen_file(
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
        messages: &ApplicationMessageSender,
        review_checkpoint: &ReviewCheckpoint,
        message: String,
    ) {
        let _ = messages.send(ReviewGuideStatusChanged {
            review_checkpoint: review_checkpoint.clone(),
            generating: false,
            message: Some(message),
        });
    }

    fn updating_status(messages: &ApplicationMessageSender, review_checkpoint: &ReviewCheckpoint) {
        let _ = messages.send(ReviewGuideStatusChanged {
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

fn guide_request_error(agent_name: &str, error: &impl std::fmt::Display) -> String {
    format!("guide request for agent {agent_name} failed: {error}")
}

#[cfg(test)]
#[path = "guide.tests.rs"]
mod tests;
