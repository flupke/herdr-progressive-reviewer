use std::fs;
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use review_hunks::{Attribution, AuthoredLines, Reviewed};
use review_types::{MarkAuthor, ReviewUnit};
use serde::{Deserialize, Serialize};

use super::{Error, Result, ReviewStore, StateKey};

/// A mark that covers the whole file at its baseline commit.
const FILE_SCHEMA_VERSION: u8 = 1;
/// A mark that covers some hunks. Older readers ignore it, so they show the
/// file as unreviewed rather than as reviewed. Who marked the lines is an
/// optional field older readers skip: a record without it was marked by the
/// reviewer.
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
    /// Who marked the whole file. A partial mark names the authors of its
    /// lines in its attribution, and repeats that attribution's default here.
    pub author: MarkAuthor,
    /// The reviewed version, when the mark covers only some lines.
    pub partial: Option<Box<PartialReview>>,
}

/// The reviewed version of a file whose mark covers only some of its lines,
/// and the base it was built on.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PartialReview {
    /// The file at the base of the reviewed comparison.
    pub base: Vec<u8>,
    /// The base with the reviewed lines applied, and who accepted them.
    pub reviewed: Reviewed,
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
    #[serde(default)]
    author: StoredAuthor,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    partial: Option<StoredPartialReview>,
}

#[derive(Debug, Deserialize, Serialize)]
struct StoredPartialReview {
    base: String,
    reviewed: String,
    #[serde(default)]
    attribution: StoredAttribution,
}

/// Who accepted the changes of a partial mark; see [`Attribution`].
#[derive(Debug, Default, Deserialize, Serialize)]
struct StoredAttribution {
    #[serde(default)]
    default: StoredAuthor,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    removed: Vec<StoredAuthoredLines>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    added: Vec<StoredAuthoredLines>,
}

#[derive(Debug, Deserialize, Serialize)]
struct StoredAuthoredLines {
    start: u32,
    end: u32,
    author: StoredAuthor,
}

/// An author kept as written, so that one a newer reviewer wrote makes its
/// record unknown rather than unreadable, and no older reviewer overwrites it.
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(transparent)]
struct StoredAuthor(Option<serde_json::Value>);

impl StoredAuthor {
    fn new(author: &MarkAuthor) -> Self {
        Self(serde_json::to_value(author).ok())
    }

    /// The author, or `None` when this reviewer does not know its kind. A
    /// record without one was marked by the reviewer.
    fn decode(&self) -> Option<MarkAuthor> {
        match &self.0 {
            None => Some(MarkAuthor::Reviewer),
            Some(value) => serde_json::from_value(value.clone()).ok(),
        }
    }
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
        author: &MarkAuthor,
    ) -> Result<ReviewRecord> {
        self.store_mark(review_unit, path, baseline_commit_id, author, None)
    }

    /// Store a mark that covers only some lines of one path.
    pub fn mark_partial(
        &self,
        review_unit: &ReviewUnit,
        path: &[u8],
        baseline_commit_id: &str,
        partial: PartialReview,
    ) -> Result<ReviewRecord> {
        let author = partial.reviewed.attribution.default.clone();
        self.store_mark(
            review_unit,
            path,
            baseline_commit_id,
            &author,
            Some(Box::new(partial)),
        )
    }

    fn store_mark(
        &self,
        review_unit: &ReviewUnit,
        path: &[u8],
        baseline_commit_id: &str,
        author: &MarkAuthor,
        partial: Option<Box<PartialReview>>,
    ) -> Result<ReviewRecord> {
        ReviewUnitKey::validate(review_unit)?;
        CommitKey::validate(baseline_commit_id)?;
        StatePath::validate(path)?;
        let record = ReviewRecord {
            path: path.to_vec(),
            baseline_commit_id: baseline_commit_id.to_owned(),
            reviewed_at: Self::timestamp("review timestamp")?,
            author: author.clone(),
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
            author: stored.author.decode().unwrap_or_default(),
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
            author: StoredAuthor::new(&record.author),
            partial: record.partial.as_ref().map(|partial| StoredPartialReview {
                base: BASE64.encode(&partial.base),
                reviewed: BASE64.encode(&partial.reviewed.text),
                attribution: StoredAttribution::from(&partial.reviewed.attribution),
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

    /// A store under `state_root` for the same repository, holding a copy of the review marks of
    /// `review_unit` and nothing else: marks written to it leave this store's alone.
    pub fn copy_marks_to(&self, review_unit: &ReviewUnit, state_root: &Path) -> Result<Self> {
        let repository = self.repository_dir.file_name().unwrap_or_default();
        let copy = Self {
            state_root: state_root.to_owned(),
            repository_dir: state_root.join(repository),
            thread_sources: std::sync::Arc::default(),
        };
        let from = self.record_directory(review_unit);
        let to = copy.record_directory(review_unit);
        let io = |operation, path: &Path| {
            let path = path.to_owned();
            move |source| Error::StateIo {
                operation,
                path,
                source,
            }
        };
        fs::create_dir_all(&to).map_err(io("create copied review marks", &to))?;
        let entries = match fs::read_dir(&from) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(copy),
            Err(source) => return Err(io("read review marks", &from)(source)),
        };
        for entry in entries {
            let entry = entry.map_err(io("read review marks", &from))?;
            if entry.file_type().is_ok_and(|kind| kind.is_file()) {
                let target = to.join(entry.file_name());
                fs::copy(entry.path(), &target).map_err(io("copy review marks", &target))?;
            }
        }
        Ok(copy)
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
        let known = match self.schema_version {
            FILE_SCHEMA_VERSION => self.partial.is_none(),
            PARTIAL_SCHEMA_VERSION => true,
            _ => false,
        };
        known && self.has_known_authors()
    }

    fn has_known_authors(&self) -> bool {
        let attribution = self.partial.as_ref().map(|partial| &partial.attribution);
        std::iter::once(&self.author)
            .chain(attribution.map(|attribution| &attribution.default))
            .chain(
                attribution
                    .into_iter()
                    .flat_map(|attribution| attribution.removed.iter().chain(&attribution.added))
                    .map(|entry| &entry.author),
            )
            .all(|author| author.decode().is_some())
    }

    /// The stored partial review, if the mark covers only some hunks.
    fn decode_partial(&self) -> std::result::Result<Option<Box<PartialReview>>, BrokenRecord> {
        if self.schema_version != PARTIAL_SCHEMA_VERSION {
            return Ok(None);
        }
        let partial = self.partial.as_ref().ok_or(BrokenRecord)?;
        let decode = |text: &str| BASE64.decode(text).map_err(|_| BrokenRecord);
        Ok(Some(Box::new(PartialReview {
            base: decode(&partial.base)?,
            reviewed: Reviewed {
                text: decode(&partial.reviewed)?,
                attribution: partial.attribution.to_attribution(),
            },
        })))
    }

    fn decode_path(&self) -> Option<Vec<u8>> {
        match self.path_encoding {
            PathEncoding::Utf8 => Some(self.path.as_bytes().to_vec()),
            PathEncoding::Base64 => BASE64.decode(&self.path).ok(),
        }
    }
}

impl From<&Attribution> for StoredAttribution {
    fn from(attribution: &Attribution) -> Self {
        let entries = |entries: &[AuthoredLines]| {
            entries
                .iter()
                .map(|entry| StoredAuthoredLines {
                    start: entry.lines.start,
                    end: entry.lines.end,
                    author: StoredAuthor::new(&entry.author),
                })
                .collect()
        };
        Self {
            default: StoredAuthor::new(&attribution.default),
            removed: entries(&attribution.removed),
            added: entries(&attribution.added),
        }
    }
}

impl StoredAttribution {
    /// The attribution, its entries sorted and without overlaps so every
    /// line has one author. Authors were checked with the schema.
    fn to_attribution(&self) -> Attribution {
        let entries = |entries: &[StoredAuthoredLines]| {
            let mut entries = entries
                .iter()
                .filter(|entry| entry.start < entry.end)
                .map(|entry| AuthoredLines {
                    lines: entry.start..entry.end,
                    author: entry.author.decode().unwrap_or_default(),
                })
                .collect::<Vec<_>>();
            entries.sort_by_key(|entry| entry.lines.start);
            let mut end = 0;
            entries.retain(|entry| {
                let separate = entry.lines.start >= end;
                if separate {
                    end = entry.lines.end;
                }
                separate
            });
            entries
        };
        Attribution {
            default: self.default.decode().unwrap_or_default(),
            removed: entries(&self.removed),
            added: entries(&self.added),
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
