//! Ask an implementation agent to delegate one review guide to a fresh subagent.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use review_guide::{
    FrozenFile, GuideResponse, GuideScope, GuideSnapshot, ReviewCheckpoint, ValidationError,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const PROMPT_TEMPLATE: &str = include_str!("write-review-guide.md");
const REPOSITORY_SNAPSHOT_FILE: &str = "repo-snapshot.json.zst";
const DIFF_FILE: &str = "diff.txt";
const RESPONSE_FILE: &str = "response.json";
const RESPONSE_TEMPORARY_FILE: &str = "response.json.new";
const RESPONSE_LIMIT: u64 = 1024 * 1024;

/// The repository state used to generate and validate one guide.
#[derive(Debug, Deserialize, Serialize)]
pub struct GuideRepositorySnapshot {
    pub repository_root: PathBuf,
    #[serde(flatten)]
    pub review_checkpoint: ReviewCheckpoint,
    pub scope: GuideScope,
    #[serde(skip)]
    pub frozen_diff: String,
    pub files: Vec<FrozenFile>,
    pub previous_items: Vec<review_guide::GuideItem>,
    pub previous_anchored_items: Vec<review_guide::AnchoredGuideItem>,
}

/// One accepted guide plus validation diagnostics.
#[derive(Debug)]
pub struct GuideResult {
    pub guide: GuideSnapshot,
    pub rejected_items: usize,
    pub response_version: GuideResponseVersion,
}

/// The observable version of one atomically published response.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GuideResponseVersion([u8; 32]);

/// The result of waiting for a mailbox response.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GuideResponseWaitOutcome {
    ResponseChanged,
    Cancelled,
}

/// A filesystem watch for one mailbox response.
pub struct GuideResponseWatch {
    mailbox: GuideMailbox,
    previous: Option<GuideResponseVersion>,
    events: Receiver<GuideResponseWatchEvent>,
    cancelled: Arc<AtomicBool>,
    _watcher: RecommendedWatcher,
}

/// Stops one mailbox response watch.
#[derive(Debug)]
pub struct GuideResponseWatchCancellation {
    cancelled: Arc<AtomicBool>,
    watch_event_sender: Sender<GuideResponseWatchEvent>,
}

enum GuideResponseWatchEvent {
    Filesystem(notify::Result<notify::Event>),
    Cancelled,
}

/// A guide operation failed without exposing prompt or repository content.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the review guide response was not written")]
    MissingResponse,
    #[error("the review guide response wait was cancelled")]
    ResponseWaitCancelled,
    #[error("the review guide response is too large")]
    LargeResponse,
    #[error("the review guide response is invalid JSON")]
    InvalidJson,
    #[error(transparent)]
    InvalidResponse(#[from] ValidationError),
    #[error("{operation} failed: {message}")]
    Operation {
        operation: &'static str,
        message: String,
    },
}

/// One repository snapshot and its deterministic response mailbox.
pub struct PreparedGuide {
    mailbox: GuideMailbox,
    repository_snapshot: GuideRepositorySnapshot,
}

impl PreparedGuide {
    /// Start an interruptible wait for the response to this prepared guide.
    pub fn watch_response(
        &self,
    ) -> Result<(GuideResponseWatch, GuideResponseWatchCancellation), Error> {
        self.mailbox.watch_response_after(None)
    }
}

/// One deterministic guide request and response directory.
#[derive(Clone, Debug)]
pub struct GuideMailbox {
    directory: PathBuf,
}

impl GuideMailbox {
    pub fn open(directory: PathBuf) -> Result<Self, Error> {
        let metadata = fs::metadata(&directory)
            .map_err(|error| operation("validate review guide mailbox", error))?;
        if !metadata.is_dir() || metadata.permissions().mode() & 0o077 != 0 {
            return Err(operation(
                "validate review guide mailbox",
                "the mailbox path is not a private directory",
            ));
        }
        Ok(Self { directory })
    }

    pub fn is_prepared(&self) -> bool {
        [REPOSITORY_SNAPSHOT_FILE, DIFF_FILE].iter().all(|name| {
            fs::symlink_metadata(self.directory.join(name)).is_ok_and(|value| value.is_file())
        })
    }

    pub fn response_version(&self) -> Result<Option<GuideResponseVersion>, Error> {
        let Some(bytes) = self.read_response()? else {
            return Ok(None);
        };
        Ok(Some(response_version(&bytes)))
    }

    pub fn watch_response_after(
        &self,
        previous: Option<GuideResponseVersion>,
    ) -> Result<(GuideResponseWatch, GuideResponseWatchCancellation), Error> {
        let (watch_event_sender, event_receiver) = mpsc::channel();
        let filesystem_event_sender = watch_event_sender.clone();
        let mut watcher = notify::recommended_watcher(move |event| {
            let event = match event {
                Ok(event) if is_response_publication_event(&event) => Ok(event),
                Ok(_) => return,
                Err(error) => Err(error),
            };
            let _ = filesystem_event_sender.send(GuideResponseWatchEvent::Filesystem(event));
        })
        .map_err(|error| operation("create review guide mailbox watch", error))?;
        watcher
            .watch(&self.directory, RecursiveMode::NonRecursive)
            .map_err(|error| operation("watch review guide mailbox", error))?;
        let cancelled = Arc::new(AtomicBool::new(false));
        Ok((
            GuideResponseWatch {
                mailbox: self.clone(),
                previous,
                events: event_receiver,
                cancelled: Arc::clone(&cancelled),
                _watcher: watcher,
            },
            GuideResponseWatchCancellation {
                cancelled,
                watch_event_sender,
            },
        ))
    }

    fn response_changed_after(
        &self,
        previous: Option<GuideResponseVersion>,
    ) -> Result<bool, Error> {
        Ok(matches!(
            self.response_version()?,
            Some(version) if Some(version) != previous
        ))
    }

    pub fn load_completed_guide(&self) -> Result<GuideResult, Error> {
        let repository_snapshot = self.read_repository_snapshot()?;
        let bytes = self.read_response()?.ok_or(Error::MissingResponse)?;
        let response_version = response_version(&bytes);
        let response: GuideResponse =
            serde_json::from_slice(&bytes).map_err(|_| Error::InvalidJson)?;
        let validated = response.validate(&repository_snapshot.files)?;
        let anchored_items = review_guide::replace_anchored_items(
            &repository_snapshot.previous_anchored_items,
            &repository_snapshot.scope,
            review_guide::anchor_items(
                &validated.items,
                &repository_snapshot.files,
                &repository_snapshot.review_checkpoint.checkpoint,
            ),
            &repository_snapshot.files,
        );
        let items = review_guide::replace_items(
            &repository_snapshot.previous_items,
            &repository_snapshot.scope,
            validated.items,
        );
        Ok(GuideResult {
            guide: GuideSnapshot {
                schema_version: 1,
                review_checkpoint: repository_snapshot.review_checkpoint,
                scope: repository_snapshot.scope,
                items,
                anchored_items,
            },
            rejected_items: validated.rejected_items,
            response_version,
        })
    }

    fn prepare(&self, repository_snapshot: &GuideRepositorySnapshot) -> Result<(), Error> {
        remove_if_present(&self.directory.join(RESPONSE_FILE))?;
        remove_if_present(&self.directory.join(RESPONSE_TEMPORARY_FILE))?;
        self.write_repository_snapshot(repository_snapshot)?;
        atomic_write_private(
            &self.directory.join(DIFF_FILE),
            repository_snapshot.frozen_diff.as_bytes(),
            "write review guide diff",
        )
    }

    fn write_repository_snapshot(
        &self,
        repository_snapshot: &GuideRepositorySnapshot,
    ) -> Result<(), Error> {
        let json = serde_json::to_vec(repository_snapshot)
            .map_err(|error| operation("encode review repository snapshot", error))?;
        let compressed = zstd::stream::encode_all(json.as_slice(), 3)
            .map_err(|error| operation("compress review repository snapshot", error))?;
        atomic_write_private(
            &self.directory.join(REPOSITORY_SNAPSHOT_FILE),
            &compressed,
            "write review repository snapshot",
        )
    }

    fn read_repository_snapshot(&self) -> Result<GuideRepositorySnapshot, Error> {
        let path = self.directory.join(REPOSITORY_SNAPSHOT_FILE);
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
            .map_err(|error| operation("open review repository snapshot", error))?;
        let mut compressed = Vec::new();
        file.read_to_end(&mut compressed)
            .map_err(|error| operation("read review repository snapshot", error))?;
        let json = zstd::stream::decode_all(compressed.as_slice())
            .map_err(|error| operation("decompress review repository snapshot", error))?;
        serde_json::from_slice(&json)
            .map_err(|error| operation("decode review repository snapshot", error))
    }

    fn read_response(&self) -> Result<Option<Vec<u8>>, Error> {
        let path = self.directory.join(RESPONSE_FILE);
        let mut file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(operation("open review guide response", error)),
        };
        let mut bytes = Vec::new();
        Read::by_ref(&mut file)
            .take(RESPONSE_LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| operation("read review guide response", error))?;
        if bytes.len() as u64 > RESPONSE_LIMIT {
            return Err(Error::LargeResponse);
        }
        Ok(Some(bytes))
    }
}

fn is_response_publication_event(event: &notify::Event) -> bool {
    if event.need_rescan() {
        return true;
    }
    if matches!(event.kind, notify::EventKind::Access(_)) {
        return false;
    }
    event
        .paths
        .iter()
        .any(|path| path.file_name().is_some_and(|name| name == RESPONSE_FILE))
}

impl GuideResponseWatch {
    pub fn wait(self) -> Result<GuideResponseWaitOutcome, Error> {
        loop {
            if self.cancelled.load(Ordering::Acquire) {
                return Ok(GuideResponseWaitOutcome::Cancelled);
            }
            if self.mailbox.response_changed_after(self.previous)? {
                return Ok(GuideResponseWaitOutcome::ResponseChanged);
            }
            match self
                .events
                .recv()
                .map_err(|error| operation("receive review guide mailbox event", error))?
            {
                GuideResponseWatchEvent::Filesystem(event) => {
                    event.map_err(|error| operation("watch review guide mailbox", error))?;
                }
                GuideResponseWatchEvent::Cancelled => {
                    return Ok(GuideResponseWaitOutcome::Cancelled);
                }
            }
        }
    }
}

impl GuideResponseWatchCancellation {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        let _ = self
            .watch_event_sender
            .send(GuideResponseWatchEvent::Cancelled);
    }
}

impl PreparedGuide {
    /// Replace the mailbox input with one repository snapshot.
    pub fn prepare(
        repository_snapshot: GuideRepositorySnapshot,
        mailbox_directory: PathBuf,
    ) -> Result<PreparedGuide, Error> {
        let mailbox = GuideMailbox::open(mailbox_directory)?;
        mailbox.prepare(&repository_snapshot)?;
        Ok(PreparedGuide {
            mailbox,
            repository_snapshot,
        })
    }

    /// Build the request for the reviewer's shared prompt dispatcher.
    pub fn prompt(&self) -> String {
        render_prompt(
            &self.repository_snapshot,
            &self.mailbox.directory.join(DIFF_FILE),
            &self.mailbox.directory.join(RESPONSE_TEMPORARY_FILE),
            &self.mailbox.directory.join(RESPONSE_FILE),
        )
    }

    /// Wait with an existing interruptible watch, then validate the response.
    pub fn finish_with_watch(&self, watch: GuideResponseWatch) -> Result<GuideResult, Error> {
        match watch.wait()? {
            GuideResponseWaitOutcome::ResponseChanged => self.mailbox.load_completed_guide(),
            GuideResponseWaitOutcome::Cancelled => Err(Error::ResponseWaitCancelled),
        }
    }
}

fn remove_if_present(path: &Path) -> Result<(), Error> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(operation("clear old review guide response", error)),
    }
}

fn atomic_write_private(
    path: &Path,
    bytes: &[u8],
    operation_name: &'static str,
) -> Result<(), Error> {
    let temporary = path.with_extension("new");
    remove_if_present(&temporary)?;
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|error| operation(operation_name, error))?;
        file.write_all(bytes)
            .map_err(|error| operation(operation_name, error))?;
        file.sync_all()
            .map_err(|error| operation(operation_name, error))?;
        fs::rename(&temporary, path).map_err(|error| operation(operation_name, error))
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn render_prompt(
    repository_snapshot: &GuideRepositorySnapshot,
    diff_path: &Path,
    response_temporary_path: &Path,
    response_path: &Path,
) -> String {
    PROMPT_TEMPLATE
        .replace(
            "{{REPOSITORY_ROOT}}",
            &repository_snapshot.repository_root.display().to_string(),
        )
        .replace(
            "{{REVIEW_SCOPE}}",
            &scope_prompt_text(&repository_snapshot.scope),
        )
        .replace("{{DIFF_PATH}}", &diff_path.display().to_string())
        .replace(
            "{{RESPONSE_TEMPORARY_PATH}}",
            &response_temporary_path.display().to_string(),
        )
        .replace("{{RESPONSE_PATH}}", &response_path.display().to_string())
}

fn scope_prompt_text(scope: &GuideScope) -> String {
    match scope {
        GuideScope::File { path } => format!("selected file `{path}`"),
        GuideScope::All => "all visible unreviewed files".to_owned(),
    }
}

fn response_version(bytes: &[u8]) -> GuideResponseVersion {
    GuideResponseVersion(Sha256::digest(bytes).into())
}

fn operation(operation: &'static str, error: impl std::fmt::Display) -> Error {
    Error::Operation {
        operation,
        message: error.to_string(),
    }
}

#[cfg(test)]
#[path = "lib.tests.rs"]
mod tests;
