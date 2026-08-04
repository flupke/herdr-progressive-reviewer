//! Atomic storage for review records and global settings.

use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

const SCHEMA_VERSION: u8 = 1;
const MAX_STATE_FILE_BYTES: u64 = 1024 * 1024;

/// A result from review storage.
pub type Result<T> = std::result::Result<T, Error>;

/// An error that does not include repository file content.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// State could not be read or changed.
    #[error("{operation} failed for review state at {path:?}: {source}")]
    StateIo {
        /// The storage operation that failed.
        operation: &'static str,
        /// The state path.
        path: PathBuf,
        /// The operating-system error.
        #[source]
        source: std::io::Error,
    },
    /// A state JSON value was invalid.
    #[error("{operation} found invalid review state at {path:?}: {source}")]
    StateJson {
        /// The storage operation that failed.
        operation: &'static str,
        /// The state path.
        path: PathBuf,
        /// The JSON error.
        #[source]
        source: serde_json::Error,
    },
    /// A value cannot be used as a safe state key.
    #[error("invalid review-state {field}")]
    InvalidStateKey {
        /// The invalid field.
        field: &'static str,
    },
    /// Two repository paths produced the same state key.
    #[error("review-state path hash collision at {path:?}")]
    StateCollision {
        /// The record path that must not be replaced.
        path: PathBuf,
    },
}

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

/// Persistent global reviewer settings.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
struct Settings {
    file_pane_width: Option<u16>,
    output_target: OutputTarget,
}

/// Where selected review text is sent.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputTarget {
    /// Insert text into the active Herdr agent.
    #[default]
    ActiveAgent,
    /// Copy text to the system clipboard.
    Clipboard,
}

/// Review state for one canonical repository.
#[derive(Debug)]
pub struct ReviewStore {
    state_root: PathBuf,
    repository_dir: PathBuf,
}

#[derive(Debug, Deserialize, Serialize)]
struct StoredRecord {
    schema_version: u8,
    change_id: String,
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

#[derive(Debug)]
struct StateKey(String);

struct ChangeKey;

struct CommitKey;

struct StatePath;

impl ReviewStore {
    /// Open the state directory for one repository.
    pub fn open(state_root: impl AsRef<Path>, repository_root: impl AsRef<Path>) -> Result<Self> {
        let repository_root =
            fs::canonicalize(repository_root.as_ref()).map_err(|source| Error::StateIo {
                operation: "canonicalize repository root",
                path: repository_root.as_ref().to_owned(),
                source,
            })?;
        let canonical_root = repository_root.to_str().ok_or(Error::InvalidStateKey {
            field: "repository root",
        })?;
        let repository_key = StateKey::hash(canonical_root.as_bytes()).0;
        let state_root = state_root.as_ref().to_owned();
        let repository_dir = state_root.join(repository_key);
        let store = Self {
            state_root,
            repository_dir,
        };
        store.create_dir(&store.repository_dir)?;
        Ok(store)
    }

    /// Store one complete review mark.
    pub fn mark(
        &self,
        change_id: &str,
        path: &[u8],
        baseline_commit_id: &str,
    ) -> Result<ReviewRecord> {
        ChangeKey::validate(change_id)?;
        CommitKey::validate(baseline_commit_id)?;
        StatePath::validate(path)?;
        let record = ReviewRecord {
            path: path.to_vec(),
            baseline_commit_id: baseline_commit_id.to_owned(),
            reviewed_at: Self::timestamp("review timestamp")?,
        };
        self.write_record(change_id, &record)?;
        Ok(record)
    }

    /// Load and validate one review mark.
    pub fn load(&self, change_id: &str, path: &[u8]) -> Result<LoadResult> {
        ChangeKey::validate(change_id)?;
        StatePath::validate(path)?;
        let target = self.record_path(change_id, path);
        let Some(stored) = Self::read_stored(&target)? else {
            return Ok(LoadResult::Unreviewed);
        };
        if stored.schema_version != SCHEMA_VERSION {
            return Ok(LoadResult::UnknownSchema);
        }
        let Some(decoded_path) = stored.decode_path() else {
            return Ok(LoadResult::Unreviewed);
        };
        let valid = stored.change_id == change_id
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
    pub fn unreview(&self, change_id: &str, path: &[u8]) -> Result<()> {
        ChangeKey::validate(change_id)?;
        StatePath::validate(path)?;
        let target = self.record_path(change_id, path);
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

    /// Get the saved file-pane width in terminal columns.
    pub fn file_pane_width(&self) -> Result<Option<u16>> {
        Ok(self.settings()?.file_pane_width)
    }

    fn settings(&self) -> Result<Settings> {
        Ok(Self::read_json(&self.settings_path(), "read settings")?.unwrap_or_default())
    }

    /// Save the file-pane width in terminal columns.
    pub fn save_file_pane_width(&self, columns: u16) -> Result<()> {
        let mut settings = self.settings()?;
        settings.file_pane_width = Some(columns);
        self.atomic_json(&self.settings_path(), &settings, "write settings")
    }

    /// Get the selected text output target.
    pub fn output_target(&self) -> Result<OutputTarget> {
        Ok(self.settings()?.output_target)
    }

    /// Save the selected text output target.
    pub fn save_output_target(&self, target: OutputTarget) -> Result<()> {
        let mut settings = self.settings()?;
        settings.output_target = target;
        self.atomic_json(&self.settings_path(), &settings, "write settings")
    }

    fn write_record(&self, change_id: &str, record: &ReviewRecord) -> Result<()> {
        let directory = self
            .repository_dir
            .join("changes")
            .join(change_id)
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
            change_id: change_id.to_owned(),
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

    fn read_json<T: DeserializeOwned>(target: &Path, operation: &'static str) -> Result<Option<T>> {
        let mut file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(target)
        {
            Ok(file) => file,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(Error::StateIo {
                    operation,
                    path: target.to_owned(),
                    source,
                });
            }
        };
        let mut bytes = Vec::new();
        Read::by_ref(&mut file)
            .take(MAX_STATE_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|source| Error::StateIo {
                operation,
                path: target.to_owned(),
                source,
            })?;
        if bytes.len() as u64 > MAX_STATE_FILE_BYTES {
            return Ok(None);
        }
        if let Ok(record) = serde_json::from_slice(&bytes) {
            Ok(Some(record))
        } else {
            Ok(None)
        }
    }

    fn atomic_json(
        &self,
        target: &Path,
        value: &impl Serialize,
        operation: &'static str,
    ) -> Result<()> {
        let mut bytes = serde_json::to_vec(value).map_err(|source| Error::StateJson {
            operation,
            path: target.to_owned(),
            source,
        })?;
        bytes.push(b'\n');
        let parent = target.parent().ok_or(Error::InvalidStateKey {
            field: "record parent",
        })?;
        self.create_dir(parent)?;
        self.remove_old_temporary_files(parent)?;
        let temporary = parent.join(format!(".tmp-{}", StateKey::random(operation, target)?.0));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temporary)
                .map_err(|source| Error::StateIo {
                    operation,
                    path: temporary.clone(),
                    source,
                })?;
            file.write_all(&bytes).map_err(|source| Error::StateIo {
                operation,
                path: temporary.clone(),
                source,
            })?;
            file.sync_all().map_err(|source| Error::StateIo {
                operation,
                path: temporary.clone(),
                source,
            })?;
            fs::rename(&temporary, target).map_err(|source| Error::StateIo {
                operation,
                path: target.to_owned(),
                source,
            })?;
            self.sync_parent(target)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    fn record_path(&self, change_id: &str, path: &[u8]) -> PathBuf {
        self.repository_dir
            .join("changes")
            .join(change_id)
            .join("paths")
            .join(format!("{}.json", StateKey::hash(path).0))
    }

    fn settings_path(&self) -> PathBuf {
        self.state_root.join("settings.json")
    }

    fn create_dir(&self, path: &Path) -> Result<()> {
        fs::create_dir_all(&self.state_root).map_err(|source| Error::StateIo {
            operation: "create review state directory",
            path: self.state_root.clone(),
            source,
        })?;
        let relative = path
            .strip_prefix(&self.state_root)
            .map_err(|_| Error::InvalidStateKey {
                field: "state directory",
            })?;
        let mut directory = self.state_root.clone();
        Self::secure_dir(&directory)?;
        for component in relative.components() {
            directory.push(component);
            match fs::create_dir(&directory) {
                Ok(()) => {}
                Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(source) => {
                    return Err(Error::StateIo {
                        operation: "create review state directory",
                        path: directory,
                        source,
                    });
                }
            }
            Self::secure_dir(&directory)?;
        }
        Ok(())
    }

    fn secure_dir(path: &Path) -> Result<()> {
        let metadata = fs::symlink_metadata(path).map_err(|source| Error::StateIo {
            operation: "inspect review state directory",
            path: path.to_owned(),
            source,
        })?;
        if !metadata.file_type().is_dir() {
            return Err(Error::InvalidStateKey {
                field: "state directory",
            });
        }
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|source| {
            Error::StateIo {
                operation: "secure review state directory",
                path: path.to_owned(),
                source,
            }
        })
    }

    fn remove_old_temporary_files(&self, directory: &Path) -> Result<()> {
        directory
            .strip_prefix(&self.state_root)
            .map_err(|_| Error::InvalidStateKey {
                field: "temporary directory",
            })?;
        let entries = fs::read_dir(directory).map_err(|source| Error::StateIo {
            operation: "scan review state directory",
            path: directory.to_owned(),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| Error::StateIo {
                operation: "scan review state directory",
                path: directory.to_owned(),
                source,
            })?;
            if !entry.file_name().as_encoded_bytes().starts_with(b".tmp-") {
                continue;
            }
            let old = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|modified| modified.elapsed().ok())
                .is_some_and(|age| age >= Duration::from_secs(24 * 60 * 60));
            if old {
                let _ = fs::remove_file(entry.path());
            }
        }
        Ok(())
    }

    fn sync_parent(&self, target: &Path) -> Result<()> {
        let parent = target.parent().ok_or(Error::InvalidStateKey {
            field: "record parent",
        })?;
        parent
            .strip_prefix(&self.state_root)
            .map_err(|_| Error::InvalidStateKey {
                field: "record parent",
            })?;
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|source| Error::StateIo {
                operation: "sync review state directory",
                path: parent.to_owned(),
                source,
            })
    }

    fn timestamp(field: &'static str) -> Result<String> {
        OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|_| Error::InvalidStateKey { field })
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

impl StateKey {
    fn hash(bytes: &[u8]) -> Self {
        Self(Self::hex(&Sha256::digest(bytes)))
    }

    fn random(operation: &'static str, path: &Path) -> Result<Self> {
        let mut bytes = [0_u8; 16];
        getrandom::fill(&mut bytes).map_err(|source| Error::StateIo {
            operation,
            path: path.to_owned(),
            source: std::io::Error::other(source.to_string()),
        })?;
        Ok(Self(Self::hex(&bytes)))
    }

    fn hex(bytes: &[u8]) -> String {
        let mut output = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            write!(&mut output, "{byte:02x}").expect("writing to a string cannot fail");
        }
        output
    }
}

impl ChangeKey {
    fn validate(value: &str) -> Result<()> {
        if !value.is_empty()
            && value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        {
            Ok(())
        } else {
            Err(Error::InvalidStateKey { field: "change ID" })
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
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};
    use std::thread;

    use tempfile::TempDir;

    use super::{LoadResult, OutputTarget, ReviewStore, StateKey};

    struct Fixture {
        temporary: TempDir,
        state: std::path::PathBuf,
        repository: std::path::PathBuf,
        change: String,
    }

    impl Fixture {
        fn new() -> Self {
            let temporary = TempDir::new().unwrap();
            let state = temporary.path().join("state");
            let repository = temporary.path().join("repository");
            fs::create_dir(&repository).unwrap();
            Self {
                temporary,
                state,
                repository,
                change: "a".repeat(64),
            }
        }

        fn store(&self) -> ReviewStore {
            ReviewStore::open(&self.state, &self.repository).unwrap()
        }
    }

    #[test]
    fn paths_round_trip_and_keys_are_stable() {
        let fixture = Fixture::new();
        let store = fixture.store();
        let baseline = "b".repeat(64);

        for path in [b"src/lib.rs".to_vec(), b"invalid-\xff".to_vec()] {
            store.mark(&fixture.change, &path, &baseline).unwrap();
            let LoadResult::Reviewed(record) = store.load(&fixture.change, &path).unwrap() else {
                panic!("record was not loaded");
            };
            assert_eq!(record.path, path);
            assert_eq!(record.baseline_commit_id, baseline);
        }

        assert_eq!(
            StateKey::hash(b"src/lib.rs").0,
            StateKey::hash(b"src/lib.rs").0
        );
    }

    #[test]
    fn roots_and_paths_have_separate_state() {
        let fixture = Fixture::new();
        let other_repository = fixture.temporary.path().join("other");
        fs::create_dir(&other_repository).unwrap();
        let first = fixture.store();
        let second = ReviewStore::open(&fixture.state, other_repository).unwrap();
        let baseline = "b".repeat(64);
        let left = b"left".to_vec();
        let right = b"right".to_vec();

        first.mark(&fixture.change, &left, &baseline).unwrap();
        first.mark(&fixture.change, &right, &baseline).unwrap();

        assert!(matches!(
            second.load(&fixture.change, &left).unwrap(),
            LoadResult::Unreviewed
        ));
        assert!(matches!(
            first.load(&fixture.change, &left).unwrap(),
            LoadResult::Reviewed(_)
        ));
        assert!(matches!(
            first.load(&fixture.change, &right).unwrap(),
            LoadResult::Reviewed(_)
        ));
    }

    #[test]
    fn settings_are_shared_between_repositories() {
        let fixture = Fixture::new();
        let other_repository = fixture.temporary.path().join("other");
        fs::create_dir(&other_repository).unwrap();
        fixture.store().save_file_pane_width(42).unwrap();
        fixture
            .store()
            .save_output_target(OutputTarget::Clipboard)
            .unwrap();

        let settings = ReviewStore::open(&fixture.state, other_repository).unwrap();
        assert_eq!(settings.file_pane_width().unwrap(), Some(42));
        assert_eq!(settings.output_target().unwrap(), OutputTarget::Clipboard);
    }

    #[test]
    fn concurrent_writers_leave_one_complete_record() {
        let fixture = Fixture::new();
        let path = b"shared".to_vec();
        fixture
            .store()
            .mark(&fixture.change, &path, &"d".repeat(64))
            .unwrap();
        let barrier = Arc::new(Barrier::new(3));
        let completed = Arc::new(AtomicUsize::new(0));
        let mut writers = Vec::new();
        for digit in ['b', 'c'] {
            let state = fixture.state.clone();
            let repository = fixture.repository.clone();
            let change = fixture.change.clone();
            let path = path.clone();
            let barrier = Arc::clone(&barrier);
            let completed = Arc::clone(&completed);
            writers.push(thread::spawn(move || {
                let store = ReviewStore::open(state, repository).unwrap();
                let baseline = digit.to_string().repeat(64);
                barrier.wait();
                for _ in 0..50 {
                    store.mark(&change, &path, &baseline).unwrap();
                }
                completed.fetch_add(1, Ordering::Release);
            }));
        }
        barrier.wait();
        let reader = fixture.store();
        while completed.load(Ordering::Acquire) != 2 {
            assert!(matches!(
                reader.load(&fixture.change, &path).unwrap(),
                LoadResult::Reviewed(_)
            ));
        }
        for writer in writers {
            writer.join().unwrap();
        }

        let LoadResult::Reviewed(record) = fixture.store().load(&fixture.change, &path).unwrap()
        else {
            panic!("record was not loaded");
        };
        assert!(
            record.baseline_commit_id == "b".repeat(64)
                || record.baseline_commit_id == "c".repeat(64)
        );
    }

    #[test]
    fn invalid_record_is_ignored_and_unreview_is_idempotent() {
        let fixture = Fixture::new();
        let store = fixture.store();
        let path = b"src/lib.rs".to_vec();
        let baseline = "b".repeat(64);
        store.mark(&fixture.change, &path, &baseline).unwrap();
        let target = store.record_path(&fixture.change, &path);
        fs::write(&target, b"{broken").unwrap();

        assert_eq!(
            store.load(&fixture.change, &path).unwrap(),
            LoadResult::Unreviewed
        );
        assert!(target.exists());
        store.unreview(&fixture.change, &path).unwrap();
        assert!(!target.exists());
        store.unreview(&fixture.change, &path).unwrap();
    }

    #[test]
    fn abandoned_temporary_file_does_not_replace_a_record() {
        let fixture = Fixture::new();
        let store = fixture.store();
        let path = b"src/lib.rs".to_vec();
        let baseline = "b".repeat(64);
        store.mark(&fixture.change, &path, &baseline).unwrap();
        let target = store.record_path(&fixture.change, &path);
        fs::write(target.parent().unwrap().join(".tmp-dead"), b"partial").unwrap();

        let LoadResult::Reviewed(record) = store.load(&fixture.change, &path).unwrap() else {
            panic!("record was not loaded");
        };
        assert_eq!(record.baseline_commit_id, baseline);
    }

    #[test]
    fn records_and_directories_are_user_only() {
        let fixture = Fixture::new();
        let store = fixture.store();
        let path = b"src/lib.rs".to_vec();
        store.mark(&fixture.change, &path, &"b".repeat(64)).unwrap();
        let target = store.record_path(&fixture.change, &path);

        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(target.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
}
