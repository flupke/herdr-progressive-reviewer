use std::path::{Path, PathBuf};

use review_guide::{GuideSnapshot, ReviewCheckpoint};
use review_types::ReviewUnit;
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
        StateKeyValue::validate(guide.review_checkpoint.review_unit.as_str(), "review unit")?;
        StateKeyValue::validate(&guide.review_checkpoint.checkpoint, "checkpoint")?;
        let snapshot = self.guide_snapshot_path(&guide.review_checkpoint);
        let stored = StoredGuideSnapshot::from_guide(guide);
        self.atomic_compressed_json(&snapshot, &stored, "write review guide")?;
        let state = StoredGuideState {
            storage_version: GUIDE_STORAGE_VERSION,
            latest_checkpoint_id: guide.review_checkpoint.checkpoint.clone(),
        };
        self.atomic_json(
            &self.guide_state_path(&guide.review_checkpoint.review_unit),
            &state,
            "write latest review guide pointer",
        )
    }

    /// Load the latest valid guide for one review unit.
    pub fn load_guide(&self, review_unit: &ReviewUnit) -> Result<Option<GuideSnapshot>> {
        StateKeyValue::validate(review_unit.as_str(), "review unit")?;
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
        {
            return Ok(None);
        }
        let snapshot = self.guide_snapshot_path(&ReviewCheckpoint::new(
            review_unit.clone(),
            state.latest_checkpoint_id,
        ));
        let guide = Self::read_bytes(&snapshot, "read review guide", None)?
            .and_then(|snapshot| Self::decode_guide_snapshot(&snapshot));
        Ok(guide.filter(|guide| {
            guide.schema_version == 1 && &guide.review_checkpoint.review_unit == review_unit
        }))
    }

    /// Return the durable mailbox for one review unit.
    pub fn guide_mailbox_directory(&self, review_unit: &ReviewUnit) -> Result<PathBuf> {
        StateKeyValue::validate(review_unit.as_str(), "review unit")?;
        let directory = self.guide_unit_directory(review_unit).join("mailbox");
        self.create_dir(&directory)?;
        Ok(directory)
    }

    fn guide_unit_directory(&self, review_unit: &ReviewUnit) -> PathBuf {
        self.repository_dir
            .join("guides")
            .join(StateKey::hash(review_unit.as_str().as_bytes()).0)
    }

    pub(super) fn guide_state_path(&self, review_unit: &ReviewUnit) -> PathBuf {
        self.guide_unit_directory(review_unit).join("state.json")
    }

    pub(super) fn guide_snapshot_path(&self, review_checkpoint: &ReviewCheckpoint) -> PathBuf {
        self.guide_unit_directory(&review_checkpoint.review_unit)
            .join("snapshots")
            .join(format!(
                "{}.json.zst",
                StateKey::hash(review_checkpoint.checkpoint.as_bytes()).0
            ))
    }

    fn decode_guide_snapshot(bytes: &[u8]) -> Option<GuideSnapshot> {
        let json = zstd::stream::decode_all(bytes).ok()?;
        serde_json::from_slice::<StoredGuideSnapshot>(&json)
            .ok()
            .and_then(StoredGuideSnapshot::into_guide)
    }

    pub(super) fn atomic_compressed_json(
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
