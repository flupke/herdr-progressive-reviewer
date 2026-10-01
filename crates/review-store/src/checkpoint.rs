use std::fs;
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use review_types::ReviewUnit;
use serde::{Deserialize, Serialize};

use super::{Error, Result, ReviewStore, StateKey};

/// A mark that covers the whole file at its baseline commit.
const FILE_SCHEMA_VERSION: u8 = 1;
/// A mark that covers some hunks. Older readers ignore it, so they show the
/// file as unreviewed rather than as reviewed.
const PARTIAL_SCHEMA_VERSION: u8 = 2;

/// One valid stored review mark.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewRecord {
    /// The exact repository-relative path.
    pub path: Vec<u8>,
    /// The commit that was reviewed.
    pub baseline_commit_id: String,
    /// The diagnostic write time.
    pub reviewed_at: String,
    /// The reviewed version, when the mark covers only some hunks.
    pub partial: Option<PartialReview>,
}

/// The reviewed version of a file whose mark covers only some of its hunks,
/// and the base it was built on.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PartialReview {
    /// The file at the base of the reviewed comparison.
    pub base: Vec<u8>,
    /// The base with the reviewed hunks applied.
    pub reviewed: Vec<u8>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    partial: Option<StoredPartialReview>,
}

#[derive(Debug, Deserialize, Serialize)]
struct StoredPartialReview {
    base: String,
    reviewed: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum PathEncoding {
    Utf8,
    Base64,
}

/// A partial mark that lost the versions it needs.
struct BrokenRecord;

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
        self.store_mark(review_unit, path, baseline_commit_id, None)
    }

    /// Store a mark that covers only some hunks of one path.
    pub fn mark_partial(
        &self,
        review_unit: &ReviewUnit,
        path: &[u8],
        baseline_commit_id: &str,
        partial: PartialReview,
    ) -> Result<ReviewRecord> {
        self.store_mark(review_unit, path, baseline_commit_id, Some(partial))
    }

    fn store_mark(
        &self,
        review_unit: &ReviewUnit,
        path: &[u8],
        baseline_commit_id: &str,
        partial: Option<PartialReview>,
    ) -> Result<ReviewRecord> {
        ReviewUnitKey::validate(review_unit)?;
        CommitKey::validate(baseline_commit_id)?;
        StatePath::validate(path)?;
        let record = ReviewRecord {
            path: path.to_vec(),
            baseline_commit_id: baseline_commit_id.to_owned(),
            reviewed_at: Self::timestamp("review timestamp")?,
            partial,
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
        if !stored.has_known_schema() {
            return Ok(LoadResult::UnknownSchema);
        }
        let (Some(decoded_path), Ok(partial)) = (stored.decode_path(), stored.decode_partial())
        else {
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
            partial,
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

    /// Clear all file review marks for one logical review, including absent paths.
    /// Conversations and other reviews are stored outside this directory.
    pub fn unreview_all(&self, review_unit: &ReviewUnit) -> Result<()> {
        ReviewUnitKey::validate(review_unit)?;
        let target = self.record_directory(review_unit);
        match fs::remove_dir_all(&target) {
            Ok(()) => self.sync_parent(&target),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(Error::StateIo {
                operation: "clear file review marks",
                path: target,
                source,
            }),
        }
    }

    fn write_record(&self, review_unit: &ReviewUnit, record: &ReviewRecord) -> Result<()> {
        let directory = self.record_directory(review_unit);
        self.create_dir(&directory)?;
        let target = directory.join(format!("{}.json", StateKey::hash(&record.path).0));
        if let Some(existing) = Self::read_stored(&target)?
            && existing.has_known_schema()
            && existing.decode_path().as_deref() != Some(record.path.as_slice())
        {
            return Err(Error::StateCollision { path: target });
        }
        let (path_encoding, path) = PathEncoding::encode(&record.path);
        let stored = StoredRecord {
            schema_version: if record.partial.is_some() {
                PARTIAL_SCHEMA_VERSION
            } else {
                FILE_SCHEMA_VERSION
            },
            review_unit: review_unit.clone(),
            path_encoding,
            path,
            baseline_commit_id: record.baseline_commit_id.clone(),
            reviewed_at: record.reviewed_at.clone(),
            partial: record.partial.as_ref().map(|partial| StoredPartialReview {
                base: BASE64.encode(&partial.base),
                reviewed: BASE64.encode(&partial.reviewed),
            }),
        };
        self.atomic_json(&target, &stored, "write review record")
    }

    /// Read one record whatever its size: a partial mark holds two versions
    /// of the file, like threads hold their source context.
    fn read_stored(target: &Path) -> Result<Option<StoredRecord>> {
        Self::read_json_within(target, "read review record", None)
    }

    pub(super) fn record_path(&self, review_unit: &ReviewUnit, path: &[u8]) -> PathBuf {
        self.record_directory(review_unit)
            .join(format!("{}.json", StateKey::hash(path).0))
    }

    fn record_directory(&self, review_unit: &ReviewUnit) -> PathBuf {
        self.repository_dir
            .join("changes")
            .join(review_unit.as_str())
            .join("paths")
    }
}

impl StoredRecord {
    fn has_known_schema(&self) -> bool {
        match self.schema_version {
            FILE_SCHEMA_VERSION => self.partial.is_none(),
            PARTIAL_SCHEMA_VERSION => true,
            _ => false,
        }
    }

    /// The stored partial review, if the mark covers only some hunks.
    fn decode_partial(&self) -> std::result::Result<Option<PartialReview>, BrokenRecord> {
        if self.schema_version != PARTIAL_SCHEMA_VERSION {
            return Ok(None);
        }
        let partial = self.partial.as_ref().ok_or(BrokenRecord)?;
        let decode = |text: &str| BASE64.decode(text).map_err(|_| BrokenRecord);
        Ok(Some(PartialReview {
            base: decode(&partial.base)?,
            reviewed: decode(&partial.reviewed)?,
        }))
    }

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

#[cfg(test)]
mod tests;
