//! Ask an implementation agent to delegate one review guide to a fresh subagent.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use herdr_client::protocol::{Agent, AgentPrompter, AgentStatus, HerdrReader};
use review_guide::{
    FrozenFile, GuideRequestId, GuideResponse, GuideScope, GuideSnapshot, ReviewCheckpoint,
    ValidationError,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const PROMPT_TEMPLATE: &str = include_str!("write-review-guide.md");
const RESPONSE_LIMIT: u64 = 1024 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(200);
const AGENT_CHANGE_GRACE: Duration = Duration::from_secs(2);
const ABANDONED_TRANSPORT_GRACE: Duration = Duration::from_secs(5 * 60);
const STORED_REQUEST_FILE: &str = "request.json.zst";

/// The complete frozen input for one guide invocation.
#[derive(Debug, Deserialize, Serialize)]
pub struct GuideRequest {
    pub repository_root: PathBuf,
    #[serde(flatten)]
    pub review_checkpoint: ReviewCheckpoint,
    pub scope: GuideScope,
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
}

/// Logical cancellation signals owned by the request coordinator.
#[derive(Debug, Default)]
pub struct GuideCancellation {
    canceled: AtomicBool,
    blocked: AtomicBool,
}

impl GuideCancellation {
    pub fn cancel(&self) {
        self.canceled.store(true, Ordering::Relaxed);
    }

    pub fn block(&self) {
        self.blocked.store(true, Ordering::Relaxed);
    }

    fn error(&self) -> Option<Error> {
        if self.blocked.load(Ordering::Relaxed) {
            Some(Error::AgentBlocked)
        } else if self.canceled.load(Ordering::Relaxed) {
            Some(Error::Canceled)
        } else {
            None
        }
    }
}

/// A guide request failed without exposing prompt or repository content.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the implementation agent changed during the guide request")]
    AgentChanged,
    #[error("the review guide request was canceled")]
    Canceled,
    #[error("the implementation agent needs user action")]
    AgentBlocked,
    #[error("the review guide response was not written")]
    MissingResponse,
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

/// Synchronous guide invocation over one typed Herdr client.
pub struct GuideRunner<'a, C> {
    client: &'a C,
}

/// One frozen request with its persistent private transport directory.
pub struct PreparedGuide {
    request_id: GuideRequestId,
    transport_directory: PathBuf,
    request: GuideRequest,
}

impl PreparedGuide {
    pub fn request_id(&self) -> &GuideRequestId {
        &self.request_id
    }

    pub fn transport_directory(&self) -> &Path {
        &self.transport_directory
    }

    /// Reopen a recorded private transport without submitting another prompt.
    pub fn resume(request_id: GuideRequestId, transport_directory: PathBuf) -> Result<Self, Error> {
        validate_transport_directory(&transport_directory)?;
        if !transport_directory.join("response.json").is_file() {
            validate_submission_marker(&transport_directory.join("submitted"))?;
        }
        let request = read_stored_request(&transport_directory.join(STORED_REQUEST_FILE))?;
        Ok(Self {
            request_id,
            transport_directory,
            request,
        })
    }
}

impl<'a, C> GuideRunner<'a, C>
where
    C: HerdrReader + AgentPrompter,
{
    /// Create a runner that waits until completion or cancellation.
    pub fn new(client: &'a C) -> Self {
        Self { client }
    }

    /// Create the private transport before prompt submission.
    pub fn prepare(request: GuideRequest) -> Result<PreparedGuide, Error> {
        let request_id = GuideRequestId::new(Uuid::new_v4().to_string());
        let transport_cleanup = TransportCleanup::new(private_transport_directory()?);
        let transport_directory = transport_cleanup.path();
        write_private(
            &transport_directory.join("repository"),
            request.repository_root.as_os_str().as_bytes(),
            "record guide transport repository",
        )?;
        write_stored_request(&transport_directory.join(STORED_REQUEST_FILE), &request)?;
        let diff_path = transport_directory.join("diff.txt");
        write_private(
            &diff_path,
            request.frozen_diff.as_bytes(),
            "write frozen guide diff",
        )?;
        Ok(PreparedGuide {
            request_id,
            transport_directory: transport_cleanup.persist(),
            request,
        })
    }

    /// Prompt one exact implementation agent and validate its response.
    #[cfg(test)]
    fn run(&self, agent: &Agent, request: GuideRequest) -> Result<GuideResult, Error> {
        let prepared = Self::prepare(request)?;
        let cancellation = GuideCancellation::default();
        self.submit_prepared(agent, &prepared, &cancellation)?;
        self.finish_prepared(agent, prepared, &cancellation)
    }

    /// Submit a prepared prompt after the final agent identity check.
    pub fn submit_prepared(
        &self,
        agent: &Agent,
        prepared: &PreparedGuide,
        cancellation: &GuideCancellation,
    ) -> Result<(), Error> {
        let transport_directory = &prepared.transport_directory;
        let diff_path = transport_directory.join("diff.txt");
        let response_path = transport_directory.join("response.json");
        let response_temporary_path = transport_directory.join("response.json.new");
        let prompt = render_prompt(
            &prepared.request,
            prepared.request_id.as_str(),
            &diff_path,
            &response_temporary_path,
            &response_path,
        );
        if let Some(error) = cancellation.error() {
            return Err(error);
        }
        let current = match self.client.get_agent(&agent.pane_id) {
            Ok(Some(current)) => current,
            Ok(None) => return Err(Error::AgentChanged),
            Err(error) => return Err(operation("resolve implementation agent", error)),
        };
        if !same_agent(agent, &current) {
            return Err(Error::AgentChanged);
        }
        if let Some(error) = cancellation.error() {
            return Err(error);
        }
        self.client
            .prompt_agent(&agent.pane_id, &prompt)
            .map_err(|error| operation("submit review guide prompt", error))?;
        write_private(
            &transport_directory.join("submitted"),
            b"",
            "record guide prompt submission",
        )
    }

    /// Finish a prompt that was submitted before the reviewer stopped.
    pub fn finish_prepared(
        &self,
        agent: &Agent,
        prepared: PreparedGuide,
        cancellation: &GuideCancellation,
    ) -> Result<GuideResult, Error> {
        let PreparedGuide {
            request_id,
            transport_directory,
            request,
        } = prepared;
        let mut transport = TransportCleanup::new(transport_directory);
        let response_path = transport.path().join("response.json");
        if let Err(error) = wait_for_response(self.client, agent, &response_path, cancellation) {
            transport.retain();
            return Err(error);
        }
        let bytes = read_bounded(&response_path)?;
        let response: GuideResponse =
            serde_json::from_slice(&bytes).map_err(|_| Error::InvalidJson)?;
        let validated = response.validate(request_id.as_str(), &request.files)?;
        let anchored_items = review_guide::replace_anchored_items(
            &request.previous_anchored_items,
            &request.scope,
            review_guide::anchor_items(
                &validated.items,
                &request.files,
                &request.review_checkpoint.checkpoint,
            ),
            &request.files,
        );
        let items =
            review_guide::replace_items(&request.previous_items, &request.scope, validated.items);
        let result = GuideResult {
            guide: GuideSnapshot {
                schema_version: 1,
                review_checkpoint: request.review_checkpoint,
                request_id,
                scope: request.scope,
                items,
                anchored_items,
            },
            rejected_items: validated.rejected_items,
        };
        Ok(result)
    }
}

/// Schedule safe cleanup for one recorded transport directory.
pub fn abandon_transport(path: PathBuf) -> Result<(), Error> {
    validate_transport_directory(&path)?;
    let _ = retain_transport_for(path, ABANDONED_TRANSPORT_GRACE);
    Ok(())
}

/// Remove expired reviewer transports that survived an earlier process.
pub fn cleanup_abandoned_transports(
    repository_root: &Path,
    protected: &[PathBuf],
) -> Result<(), Error> {
    let temporary_root = std::env::temp_dir();
    let entries = fs::read_dir(&temporary_root)
        .map_err(|error| operation("scan abandoned guide transports", error))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let is_reviewer_transport = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("herdr-review-guide-"));
        if !is_reviewer_transport
            || !transport_belongs_to_repository(&path, repository_root)
            || protected.contains(&path)
            || validate_transport_directory(&path).is_err()
        {
            continue;
        }
        let Ok(modified) = entry.metadata().and_then(|metadata| metadata.modified()) else {
            continue;
        };
        if modified
            .elapsed()
            .is_ok_and(|age| age >= ABANDONED_TRANSPORT_GRACE)
        {
            let _ = fs::remove_dir_all(path);
        }
    }
    Ok(())
}

fn transport_belongs_to_repository(path: &Path, repository_root: &Path) -> bool {
    let Ok(mut file) = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path.join("repository"))
    else {
        return false;
    };
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(4097)
        .read_to_end(&mut bytes)
        .is_ok()
        && bytes.len() <= 4096
        && bytes == repository_root.as_os_str().as_bytes()
}

fn same_agent(original: &Agent, current: &Agent) -> bool {
    let stable_fields_match = original.pane_id == current.pane_id
        && original.tab_id == current.tab_id
        && original.workspace_id == current.workspace_id
        && original.agent == current.agent
        && original.cwd == current.cwd;
    stable_fields_match
        && match (&original.agent_session, &current.agent_session) {
            (Some(original), Some(current)) => original == current,
            (None, None) => true,
            _ => false,
        }
}

struct TransportCleanup {
    path: Option<PathBuf>,
}

impl TransportCleanup {
    fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    fn path(&self) -> &Path {
        self.path.as_deref().expect("transport cleanup owns a path")
    }

    fn retain(&mut self) {
        if let Some(path) = self.path.take() {
            let _ = retain_transport_for(path, ABANDONED_TRANSPORT_GRACE);
        }
    }

    fn persist(mut self) -> PathBuf {
        self.path.take().expect("transport cleanup owns a path")
    }
}

impl Drop for TransportCleanup {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            let _ = fs::remove_dir_all(path);
        }
    }
}

fn retain_transport_for(path: PathBuf, retention: Duration) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        thread::sleep(retention);
        let _ = fs::remove_dir_all(path);
    })
}

fn private_transport_directory() -> Result<PathBuf, Error> {
    let directory = tempfile::Builder::new()
        .prefix("herdr-review-guide-")
        .tempdir_in(std::env::temp_dir())
        .map_err(|error| operation("create guide transport directory", error))?;
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))
        .map_err(|error| operation("secure guide transport directory", error))?;
    Ok(directory.keep())
}

fn validate_transport_directory(path: &Path) -> Result<(), Error> {
    let temporary_root = fs::canonicalize(std::env::temp_dir())
        .map_err(|error| operation("validate guide transport directory", error))?;
    let canonical = fs::canonicalize(path)
        .map_err(|error| operation("validate guide transport directory", error))?;
    let valid_name = canonical
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("herdr-review-guide-"));
    let metadata = fs::symlink_metadata(&canonical)
        .map_err(|error| operation("validate guide transport directory", error))?;
    let valid = canonical.parent() == Some(temporary_root.as_path())
        && valid_name
        && metadata.file_type().is_dir()
        && metadata.uid() == rustix::process::geteuid().as_raw()
        && metadata.permissions().mode().trailing_zeros() >= 6;
    if !valid {
        return Err(operation(
            "validate guide transport directory",
            "the recorded path is not a private reviewer transport",
        ));
    }
    Ok(())
}

fn validate_submission_marker(path: &Path) -> Result<(), Error> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| operation("validate guide submission record", error))?;
    if !metadata.file_type().is_file() || metadata.permissions().mode().trailing_zeros() < 6 {
        return Err(operation(
            "validate guide submission record",
            "the request was not confirmed as submitted",
        ));
    }
    Ok(())
}

fn write_private(path: &Path, bytes: &[u8], operation_name: &'static str) -> Result<(), Error> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| operation(operation_name, error))?;
    file.write_all(bytes)
        .map_err(|error| operation(operation_name, error))
}

fn render_prompt(
    request: &GuideRequest,
    request_id: &str,
    diff_path: &Path,
    response_temporary_path: &Path,
    response_path: &Path,
) -> String {
    PROMPT_TEMPLATE
        .replace("{{REQUEST_ID}}", request_id)
        .replace(
            "{{REPOSITORY_ROOT}}",
            &request.repository_root.display().to_string(),
        )
        .replace("{{REVIEW_SCOPE}}", &scope_prompt_text(&request.scope))
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

fn wait_for_response(
    client: &impl HerdrReader,
    original: &Agent,
    response_path: &Path,
    cancellation: &GuideCancellation,
) -> Result<(), Error> {
    let mut agent_change_deadline = None;
    loop {
        if response_path.is_file() {
            return Ok(());
        }
        if let Some(error) = cancellation.error() {
            return Err(error);
        }
        let agent = client
            .get_agent(&original.pane_id)
            .map_err(|error| operation("observe implementation agent", error))?;
        if let Some(agent) = agent.filter(|agent| same_agent(original, agent)) {
            agent_change_deadline = None;
            if agent.agent_status == AgentStatus::Blocked {
                return Err(Error::AgentBlocked);
            }
        } else {
            let change_deadline =
                *agent_change_deadline.get_or_insert_with(|| Instant::now() + AGENT_CHANGE_GRACE);
            if Instant::now() >= change_deadline {
                return Err(Error::AgentChanged);
            }
        }
        thread::sleep(POLL_INTERVAL);
    }
}

fn write_stored_request(path: &Path, request: &GuideRequest) -> Result<(), Error> {
    let json = serde_json::to_vec(request)
        .map_err(|error| operation("encode frozen guide request", error))?;
    let compressed = zstd::stream::encode_all(json.as_slice(), 3)
        .map_err(|error| operation("compress frozen guide request", error))?;
    write_private(path, &compressed, "write frozen guide request")
}

fn read_stored_request(path: &Path) -> Result<GuideRequest, Error> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|error| operation("open frozen guide request", error))?;
    let mut compressed = Vec::new();
    file.read_to_end(&mut compressed)
        .map_err(|error| operation("read frozen guide request", error))?;
    let json = zstd::stream::decode_all(compressed.as_slice())
        .map_err(|error| operation("decompress frozen guide request", error))?;
    serde_json::from_slice(&json).map_err(|error| operation("decode frozen guide request", error))
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, Error> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| Error::MissingResponse)?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(RESPONSE_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| operation("read review guide response", error))?;
    if bytes.len() as u64 > RESPONSE_LIMIT {
        return Err(Error::LargeResponse);
    }
    Ok(bytes)
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
