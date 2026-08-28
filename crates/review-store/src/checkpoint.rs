use std::fs;
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use review_types::ReviewUnit;
use serde::{Deserialize, Serialize};

use super::{Error, Result, ReviewStore, StateKey};

const SCHEMA_VERSION: u8 = 1;

/// One valid stored review mark.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewRecord {
    /// The exact repository-relative path.
    pub path: Vec<u8>,
    /// The commit that was reviewed.
    pub baseline_commit_id: String,
    /// The diagnostic write time.
    pub reviewed_at: String,
}

/// The result of loading one path record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LoadResult {
    /// No usable record exists.
    Unreviewed,
    /// A valid record exists.
    Reviewed(ReviewRecord),
    /// A record was ignored because its schema version is unknown.
    UnknownSchema,
}

#[derive(Debug, Deserialize, Serialize)]
struct StoredRecord {
    schema_version: u8,
    #[serde(alias = "change_id")]
    review_unit: ReviewUnit,
    path_encoding: PathEncoding,
    path: String,
    baseline_commit_id: String,
    reviewed_at: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum PathEncoding {
    Utf8,
    Base64,
}

struct ReviewUnitKey;

struct CommitKey;

struct StatePath;

impl ReviewStore {
    /// Store one complete review mark.
    pub fn mark(
        &self,
        review_unit: &ReviewUnit,
        path: &[u8],
        baseline_commit_id: &str,
    ) -> Result<ReviewRecord> {
        ReviewUnitKey::validate(review_unit)?;
        CommitKey::validate(baseline_commit_id)?;
        StatePath::validate(path)?;
        let record = ReviewRecord {
            path: path.to_vec(),
            baseline_commit_id: baseline_commit_id.to_owned(),
            reviewed_at: Self::timestamp("review timestamp")?,
        };
        self.write_record(review_unit, &record)?;
        Ok(record)
    }

    /// Load and validate one review mark.
    pub fn load(&self, review_unit: &ReviewUnit, path: &[u8]) -> Result<LoadResult> {
        ReviewUnitKey::validate(review_unit)?;
        StatePath::validate(path)?;
        let target = self.record_path(review_unit, path);
        let Some(stored) = Self::read_stored(&target)? else {
            return Ok(LoadResult::Unreviewed);
        };
        if stored.schema_version != SCHEMA_VERSION {
            return Ok(LoadResult::UnknownSchema);
        }
        let Some(decoded_path) = stored.decode_path() else {
            return Ok(LoadResult::Unreviewed);
        };
        let valid = &stored.review_unit == review_unit
            && decoded_path == path
            && CommitKey::is_valid(&stored.baseline_commit_id);
        if !valid {
            return Ok(LoadResult::Unreviewed);
        }
        Ok(LoadResult::Reviewed(ReviewRecord {
            path: decoded_path,
            baseline_commit_id: stored.baseline_commit_id,
            reviewed_at: stored.reviewed_at,
        }))
    }

    /// Remove one review mark. A missing record is success.
    pub fn unreview(&self, review_unit: &ReviewUnit, path: &[u8]) -> Result<()> {
        ReviewUnitKey::validate(review_unit)?;
        StatePath::validate(path)?;
        let target = self.record_path(review_unit, path);
        match fs::remove_file(&target) {
            Ok(()) => self.sync_parent(&target),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(Error::StateIo {
                operation: "remove review record",
                path: target,
                source,
            }),
        }
    }

    fn write_record(&self, review_unit: &ReviewUnit, record: &ReviewRecord) -> Result<()> {
        let directory = self
            .repository_dir
            .join("changes")
            .join(review_unit.as_str())
            .join("paths");
        self.create_dir(&directory)?;
        let target = directory.join(format!("{}.json", StateKey::hash(&record.path).0));
        if let Some(existing) = Self::read_stored(&target)?
            && existing.schema_version == SCHEMA_VERSION
            && existing.decode_path().as_deref() != Some(record.path.as_slice())
        {
            return Err(Error::StateCollision { path: target });
        }
        let (path_encoding, path) = PathEncoding::encode(&record.path);
        let stored = StoredRecord {
            schema_version: SCHEMA_VERSION,
            review_unit: review_unit.clone(),
            path_encoding,
            path,
            baseline_commit_id: record.baseline_commit_id.clone(),
            reviewed_at: record.reviewed_at.clone(),
        };
        self.atomic_json(&target, &stored, "write review record")
    }

    fn read_stored(target: &Path) -> Result<Option<StoredRecord>> {
        Self::read_json(target, "read review record")
    }

    pub(super) fn record_path(&self, review_unit: &ReviewUnit, path: &[u8]) -> PathBuf {
        self.repository_dir
            .join("changes")
            .join(review_unit.as_str())
            .join("paths")
            .join(format!("{}.json", StateKey::hash(path).0))
    }
}

impl StoredRecord {
    fn decode_path(&self) -> Option<Vec<u8>> {
        match self.path_encoding {
            PathEncoding::Utf8 => Some(self.path.as_bytes().to_vec()),
            PathEncoding::Base64 => BASE64.decode(&self.path).ok(),
        }
    }
}

impl PathEncoding {
    fn encode(path: &[u8]) -> (Self, String) {
        match std::str::from_utf8(path) {
            Ok(path) => (Self::Utf8, path.to_owned()),
            Err(_) => (Self::Base64, BASE64.encode(path)),
        }
    }
}

impl ReviewUnitKey {
    fn validate(value: &ReviewUnit) -> Result<()> {
        let value = value.as_str();
        if !value.is_empty()
            && value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        {
            Ok(())
        } else {
            Err(Error::InvalidStateKey {
                field: "review unit",
            })
        }
    }
}

impl CommitKey {
    fn validate(value: &str) -> Result<()> {
        if Self::is_valid(value) {
            Ok(())
        } else {
            Err(Error::InvalidStateKey { field: "commit ID" })
        }
    }

    fn is_valid(value: &str) -> bool {
        !value.is_empty()
            && value
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    }
}

impl StatePath {
    fn validate(bytes: &[u8]) -> Result<()> {
        if bytes.is_empty()
            || bytes[0] == b'/'
            || bytes
                .split(|byte| *byte == b'/')
                .any(|part| part.is_empty() || part == b"." || part == b"..")
        {
            return Err(Error::InvalidStateKey { field: "path" });
        }
        Ok(())
    }
}
