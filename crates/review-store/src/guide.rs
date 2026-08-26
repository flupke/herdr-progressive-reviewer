use std::fs;
use std::path::{Path, PathBuf};

use herdr_client::protocol::Agent;
use review_guide::{GuideRequestId, GuideScope, GuideSnapshot, ReviewCheckpoint};
use serde::{Deserialize, Serialize};

use super::{Error, Result, ReviewStore, StateKey};

const GUIDE_STORAGE_VERSION: u8 = 1;
const GUIDE_COMPRESSION_LEVEL: i32 = 3;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct GuideAnchorContent {
    old_content: Option<Vec<u8>>,
    new_content: Option<Vec<u8>>,
}

#[derive(Debug, Deserialize, Serialize)]
struct StoredGuideSnapshot {
    storage_version: u8,
    guide: GuideSnapshot,
    anchor_content_indices: Vec<usize>,
    anchor_contents: Vec<GuideAnchorContent>,
}

#[derive(Debug, Deserialize, Serialize)]
struct StoredGuideState {
    storage_version: u8,
    latest_checkpoint_id: String,
    latest_request_id: GuideRequestId,
}

/// The persisted lifecycle state of one manual guide request.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuideRequestState {
    Preparing,
    Submitted,
    Incomplete,
    Completed,
    Failed,
}

/// Durable metadata for one guide request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GuideRequestRecord {
    pub schema_version: u8,
    #[serde(flatten)]
    pub review_checkpoint: ReviewCheckpoint,
    pub request_id: GuideRequestId,
    pub scope: GuideScope,
    pub agent: Agent,
    pub state: GuideRequestState,
    pub created_at: String,
    pub updated_at: String,
    pub transport_directory: Option<PathBuf>,
    pub error: Option<String>,
}

impl StoredGuideSnapshot {
    fn from_guide(guide: &GuideSnapshot) -> Self {
        let mut stored_guide = guide.clone();
        let mut anchor_contents = Vec::new();
        let anchor_content_indices = stored_guide
            .anchored_items
            .iter_mut()
            .map(|item| {
                let content = GuideAnchorContent {
                    old_content: item.anchor.old_content.take(),
                    new_content: item.anchor.new_content.take(),
                };
                anchor_contents
                    .iter()
                    .position(|candidate| candidate == &content)
                    .unwrap_or_else(|| {
                        anchor_contents.push(content);
                        anchor_contents.len() - 1
                    })
            })
            .collect();
        Self {
            storage_version: GUIDE_STORAGE_VERSION,
            guide: stored_guide,
            anchor_content_indices,
            anchor_contents,
        }
    }

    fn into_guide(mut self) -> Option<GuideSnapshot> {
        if self.storage_version != GUIDE_STORAGE_VERSION
            || self.guide.anchored_items.len() != self.anchor_content_indices.len()
        {
            return None;
        }
        for (item, content_index) in self
            .guide
            .anchored_items
            .iter_mut()
            .zip(self.anchor_content_indices)
        {
            let content = self.anchor_contents.get(content_index)?;
            item.anchor.old_content.clone_from(&content.old_content);
            item.anchor.new_content.clone_from(&content.new_content);
        }
        Some(self.guide)
    }
}

impl ReviewStore {
    /// Store one complete guide and make it the latest guide for its review unit.
    pub fn save_guide(&self, guide: &GuideSnapshot) -> Result<()> {
        StateKeyValue::validate(&guide.review_checkpoint.review_unit, "review unit")?;
        StateKeyValue::validate(&guide.review_checkpoint.checkpoint, "checkpoint")?;
        StateKeyValue::validate(guide.request_id.as_str(), "request ID")?;
        let snapshot = self.guide_snapshot_path(&guide.review_checkpoint, &guide.request_id);
        let stored = StoredGuideSnapshot::from_guide(guide);
        self.atomic_compressed_json(&snapshot, &stored, "write review guide")?;
        let state = StoredGuideState {
            storage_version: GUIDE_STORAGE_VERSION,
            latest_checkpoint_id: guide.review_checkpoint.checkpoint.clone(),
            latest_request_id: guide.request_id.clone(),
        };
        self.atomic_json(
            &self.guide_state_path(&guide.review_checkpoint.review_unit),
            &state,
            "write latest review guide pointer",
        )
    }

    /// Load the latest valid guide for one review unit.
    pub fn load_guide(&self, review_unit: &str) -> Result<Option<GuideSnapshot>> {
        StateKeyValue::validate(review_unit, "review unit")?;
        let target = self.guide_state_path(review_unit);
        let Some(bytes) = Self::read_bytes(&target, "read latest review guide pointer", None)?
        else {
            return Ok(None);
        };
        let Ok(state) = serde_json::from_slice::<StoredGuideState>(&bytes) else {
            return Ok(None);
        };
        if state.storage_version != GUIDE_STORAGE_VERSION
            || StateKeyValue::validate(&state.latest_checkpoint_id, "checkpoint").is_err()
            || StateKeyValue::validate(state.latest_request_id.as_str(), "request ID").is_err()
        {
            return Ok(None);
        }
        let snapshot = self.guide_snapshot_path(
            &ReviewCheckpoint::new(review_unit, state.latest_checkpoint_id),
            &state.latest_request_id,
        );
        let guide = Self::read_bytes(&snapshot, "read review guide", None)?
            .and_then(|snapshot| Self::decode_guide_snapshot(&snapshot));
        Ok(guide.filter(|guide| {
            guide.schema_version == 1 && guide.review_checkpoint.review_unit == review_unit
        }))
    }

    /// Store the current state of one guide request and keep its history.
    pub fn save_guide_request(&self, request: &mut GuideRequestRecord) -> Result<()> {
        StateKeyValue::validate(&request.review_checkpoint.review_unit, "review unit")?;
        StateKeyValue::validate(&request.review_checkpoint.checkpoint, "checkpoint")?;
        StateKeyValue::validate(request.request_id.as_str(), "request ID")?;
        let now = Self::timestamp("guide request timestamp")?;
        if request.created_at.is_empty() {
            request.created_at.clone_from(&now);
        }
        request.updated_at = now;
        let target = self
            .guide_unit_directory(&request.review_checkpoint.review_unit)
            .join("requests")
            .join(format!(
                "{}.json",
                StateKey::hash(request.request_id.as_bytes()).0
            ));
        self.atomic_json(&target, request, "write review guide request")
    }

    /// Load every valid request record for one review unit.
    pub fn load_guide_requests(&self, review_unit: &str) -> Result<Vec<GuideRequestRecord>> {
        StateKeyValue::validate(review_unit, "review unit")?;
        let directory = self.guide_unit_directory(review_unit).join("requests");
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => {
                return Err(Error::StateIo {
                    operation: "scan review guide requests",
                    path: directory,
                    source,
                });
            }
        };
        let mut requests = Vec::new();
        for entry in entries.flatten() {
            let Some(request): Option<GuideRequestRecord> =
                Self::read_json(&entry.path(), "read review guide request")?
            else {
                continue;
            };
            if request.schema_version == 1 && request.review_checkpoint.review_unit == review_unit {
                requests.push(request);
            }
        }
        requests.sort_by(|left, right| left.created_at.cmp(&right.created_at));
        Ok(requests)
    }

    /// List transports that a durable request can still use.
    pub fn active_guide_transport_directories(&self) -> Result<Vec<PathBuf>> {
        let guides = self.repository_dir.join("guides");
        let units = match fs::read_dir(&guides) {
            Ok(units) => units,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => {
                return Err(Error::StateIo {
                    operation: "scan review guide units",
                    path: guides,
                    source,
                });
            }
        };
        let mut transports = Vec::new();
        for unit in units.flatten() {
            let requests = unit.path().join("requests");
            let Ok(entries) = fs::read_dir(requests) else {
                continue;
            };
            for entry in entries.flatten() {
                let Some(request): Option<GuideRequestRecord> =
                    Self::read_json(&entry.path(), "read review guide request")?
                else {
                    continue;
                };
                if matches!(
                    request.state,
                    GuideRequestState::Preparing
                        | GuideRequestState::Submitted
                        | GuideRequestState::Incomplete
                ) && let Some(path) = request.transport_directory
                {
                    transports.push(path);
                }
            }
        }
        Ok(transports)
    }

    fn guide_unit_directory(&self, review_unit: &str) -> PathBuf {
        self.repository_dir
            .join("guides")
            .join(StateKey::hash(review_unit.as_bytes()).0)
    }

    pub(super) fn guide_state_path(&self, review_unit: &str) -> PathBuf {
        self.guide_unit_directory(review_unit).join("state.json")
    }

    pub(super) fn guide_snapshot_path(
        &self,
        review_checkpoint: &ReviewCheckpoint,
        request_id: &GuideRequestId,
    ) -> PathBuf {
        self.guide_unit_directory(&review_checkpoint.review_unit)
            .join("snapshots")
            .join(StateKey::hash(review_checkpoint.checkpoint.as_bytes()).0)
            .join(format!(
                "{}.json.zst",
                StateKey::hash(request_id.as_bytes()).0
            ))
    }

    fn decode_guide_snapshot(bytes: &[u8]) -> Option<GuideSnapshot> {
        let json = zstd::stream::decode_all(bytes).ok()?;
        serde_json::from_slice::<StoredGuideSnapshot>(&json)
            .ok()
            .and_then(StoredGuideSnapshot::into_guide)
    }

    fn atomic_compressed_json(
        &self,
        target: &Path,
        value: &impl Serialize,
        operation: &'static str,
    ) -> Result<()> {
        let json = serde_json::to_vec(value).map_err(|source| Error::StateJson {
            operation,
            path: target.to_owned(),
            source,
        })?;
        let bytes = zstd::stream::encode_all(json.as_slice(), GUIDE_COMPRESSION_LEVEL).map_err(
            |source| Error::StateIo {
                operation,
                path: target.to_owned(),
                source,
            },
        )?;
        self.atomic_write(target, &bytes, operation)
    }
}

struct StateKeyValue;

impl StateKeyValue {
    fn validate(value: &str, field: &'static str) -> Result<()> {
        if !value.is_empty()
            && value.len() <= 256
            && value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':')
            })
        {
            Ok(())
        } else {
            Err(Error::InvalidStateKey { field })
        }
    }
}
