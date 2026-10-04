//! The Explore record format and exclusive, locked access to one review's records.
//!
//! Each review keeps a history index, one file per round, one editor view per round, and the
//! record of the run-ahead forks of each round that had some.
//! The store only reads, writes and removes them; what a change means belongs to the
//! Explore session.
use super::{Error, Result, ReviewStore, StateKey};
use fs2::FileExt;
use review_explore::{ExploreHistory, ExploreRound, ViewSave};
use review_run_ahead::RoundForks;
use review_types::ReviewUnit;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    fs::{self, OpenOptions},
    io::Read,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    time::SystemTime,
};

/// Version 1 records had coverage, inspections, deferrals and corrections;
/// they are no longer loaded, as if absent.
const VERSION: u32 = 2;
// A round grows across many valid 1 MiB submissions. This is not a source archive.
const MAX_DOMAIN: u64 = 256 * 1024 * 1024;
const MAX_VIEW: u64 = 16 * 1024 * 1024;
// The forks of a round: a few small records per question.
const MAX_FORKS: u64 = 16 * 1024 * 1024;

#[derive(Deserialize, Serialize)]
struct Stored<T> {
    version: u32,
    value: T,
}

/// Only the version of a stored record, read before its value.
#[derive(Deserialize)]
struct StoredVersion {
    version: u32,
}

/// One review's Explore records, exclusively locked until this value is dropped.
///
/// Every read-modify-write of a review's records happens through one of these, so
/// callers never save a stale copy over a concurrent change.
pub struct ExploreRecords<'a> {
    store: &'a ReviewStore,
    unit: ReviewUnit,
    directory: PathBuf,
    _lock: fs::File,
}

impl ExploreRecords<'_> {
    pub fn history(&self) -> Result<ExploreHistory> {
        self.store.load_explore_history(&self.unit)
    }

    pub fn save_history(&self, history: &ExploreHistory) -> Result<()> {
        self.store
            .write_explore(&self.directory.join("index.json"), history, MAX_VIEW)
    }

    pub fn round(&self, instance: &str) -> Result<Option<ExploreRound>> {
        self.store.load_explore(&self.unit, instance)
    }

    /// Save a round that has no record yet. A saved round only changes through
    /// [`Self::update_round`], so no caller writes a stale whole-round copy.
    pub fn create_round(&self, round: &ExploreRound) -> Result<()> {
        if self.round(&round.exploration.instance)?.is_some() {
            return Err(Error::Explore("round already exists".into()));
        }
        self.save_round(round)
    }

    fn save_round(&self, round: &ExploreRound) -> Result<()> {
        if round.exploration.comparison.checkpoint.review_unit != self.unit {
            return Err(Error::Explore("round belongs to another review".into()));
        }
        self.store.write_explore(
            &self
                .store
                .explore_path(&self.unit, &round.exploration.instance)?,
            round,
            MAX_DOMAIN,
        )
    }

    /// Change a saved round, writing it with the next revision only when it changed.
    pub fn update_round<T>(
        &self,
        instance: &str,
        update: impl FnOnce(&mut ExploreRound) -> std::result::Result<T, String>,
    ) -> Result<(T, ExploreRound)> {
        let mut round = self
            .round(instance)?
            .ok_or_else(|| Error::Explore("saved round is missing".into()))?;
        let original = round.clone();
        let result = update(&mut round).map_err(Error::ExploreRefused)?;
        if round != original {
            round.revision = round
                .revision
                .checked_add(1)
                .ok_or_else(|| Error::Explore("revision exhausted".into()))?;
            self.save_round(&round)?;
        }
        Ok((result, round))
    }

    pub fn view(&self, instance: &str) -> Result<Option<ViewSave>> {
        self.store.load_explore_view(&self.unit, instance)
    }

    pub fn save_view(&self, view: &ViewSave) -> Result<()> {
        if view.review_unit != self.unit {
            return Err(Error::Explore("editor belongs to another review".into()));
        }
        self.store.write_explore(
            &self.store.explore_view_path(&self.unit, &view.instance)?,
            view,
            MAX_VIEW,
        )
    }

    /// The run-ahead forks of the round `instance`; none when it never had any.
    fn round_forks(&self, instance: &str) -> Result<RoundForks> {
        self.store.load_round_forks(&self.unit, instance)
    }

    /// Change the record of the run-ahead forks of the round `instance`, whether or not the
    /// round is still the editable latest one: forks are processes and files to clean up, not
    /// decisions of the round.
    pub fn update_round_forks<T>(
        &self,
        instance: &str,
        update: impl FnOnce(&mut RoundForks) -> T,
    ) -> Result<(T, RoundForks)> {
        let mut forks = self.round_forks(instance)?;
        let original = forks.clone();
        let result = update(&mut forks);
        if forks != original {
            self.store.write_explore(
                &self.store.round_forks_path(&self.unit, instance)?,
                &forks,
                MAX_FORKS,
            )?;
        }
        Ok((result, forks))
    }

    /// Remove a round file; a missing file is already removed.
    pub fn remove_round(&self, instance: &str) -> Result<()> {
        ReviewStore::remove_explore_file(&self.store.explore_path(&self.unit, instance)?)
    }

    /// Remove the record of a round's run-ahead forks; a missing file is already removed.
    pub fn remove_round_forks(&self, instance: &str) -> Result<()> {
        ReviewStore::remove_explore_file(&self.store.round_forks_path(&self.unit, instance)?)
    }

    /// Remove an editor view file; a missing file is already removed.
    pub fn remove_view(&self, instance: &str) -> Result<()> {
        ReviewStore::remove_explore_file(&self.store.explore_view_path(&self.unit, instance)?)
    }

    /// Make earlier removals durable.
    pub fn sync(&self) -> Result<()> {
        self.store.sync_parent(&self.directory.join("index.json"))
    }

    /// The identity and modification time of every round file, readable or not.
    pub fn round_files(&self) -> Result<Vec<(String, SystemTime)>> {
        ReviewStore::round_files_in(&self.directory)
    }
}

/// Every round saved for one repository, read without changing anything.
#[derive(Debug, Default)]
pub struct SavedRounds {
    pub rounds: Vec<SavedRound>,
    /// Round files saved in an earlier format, or damaged.
    pub unreadable: usize,
}

/// A saved round and when its file was last written.
#[derive(Debug)]
pub struct SavedRound {
    pub round: ExploreRound,
    pub saved_at: SystemTime,
}

impl ReviewStore {
    /// Watch this namespace with the existing filesystem event infrastructure.
    pub fn explore_directory(&self) -> PathBuf {
        self.repository_dir.join("explore-v1")
    }

    /// Read every saved round of every review of this repository. It takes no
    /// lock: a round file is replaced atomically, so each read sees one whole
    /// version. A missing Explore directory holds no rounds.
    pub fn saved_explore_rounds(&self) -> Result<SavedRounds> {
        let mut saved = SavedRounds::default();
        let directory = self.explore_directory();
        if !directory.exists() {
            return Ok(saved);
        }
        for review in Self::explore_entries(&directory)? {
            let review = review?.path();
            if !review.is_dir() {
                continue;
            }
            for (instance, saved_at) in Self::round_files_in(&review)? {
                match Self::read_explore(&review.join(format!("{instance}.json")), MAX_DOMAIN) {
                    Ok(Some(round)) => saved.rounds.push(SavedRound { round, saved_at }),
                    Ok(None) | Err(_) => saved.unreadable += 1,
                }
            }
        }
        Ok(saved)
    }

    /// The review and the identity of every round of every review of this repository whose
    /// run-ahead forks `wanted` picks, such as forks a stopped reviewer left. It takes no lock,
    /// as [`Self::saved_explore_rounds`]; a record it cannot read is left out.
    pub fn rounds_with_forks(
        &self,
        wanted: impl Fn(&RoundForks) -> bool,
    ) -> Result<Vec<(ReviewUnit, String)>> {
        let mut rounds = Vec::new();
        let directory = self.explore_directory();
        if !directory.exists() {
            return Ok(rounds);
        }
        for review in Self::explore_entries(&directory)? {
            let review = review?.path();
            if !review.is_dir() {
                continue;
            }
            for entry in Self::explore_entries(&review)? {
                let entry = entry?;
                let Some(instance) = entry
                    .file_name()
                    .to_str()
                    .and_then(|name| name.strip_suffix(".forks.json"))
                    .filter(|instance| Self::explore_id(instance).is_ok())
                    .map(str::to_owned)
                else {
                    continue;
                };
                let picked = Self::read_explore::<RoundForks>(&entry.path(), MAX_FORKS)
                    .ok()
                    .flatten()
                    .is_some_and(|forks| wanted(&forks));
                if !picked {
                    continue;
                }
                let round = review.join(format!("{instance}.json"));
                if let Ok(Some(round)) = Self::read_explore::<ExploreRound>(&round, MAX_DOMAIN) {
                    let unit = round.exploration.comparison.checkpoint.review_unit.clone();
                    rounds.push((unit, instance));
                }
            }
        }
        Ok(rounds)
    }

    /// The identity and modification time of every round file in one review's
    /// directory, readable or not.
    fn round_files_in(directory: &Path) -> Result<Vec<(String, SystemTime)>> {
        let mut rounds = Vec::new();
        for entry in Self::explore_entries(directory)? {
            let entry = entry?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Some(instance) = name.strip_suffix(".json") else {
                continue;
            };
            if instance == "index" || Self::explore_id(instance).is_err() {
                continue;
            }
            let modified = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            rounds.push((instance.to_owned(), modified));
        }
        Ok(rounds)
    }

    pub fn prepare_explore_storage(&self) -> Result<()> {
        self.create_dir(&self.explore_directory())
    }

    fn explore_review(&self, unit: &ReviewUnit) -> Result<PathBuf> {
        if unit.is_empty() {
            return Err(Error::Explore("missing logical review identity".into()));
        }
        Ok(self
            .explore_directory()
            .join(StateKey::hash(unit.as_str().as_bytes()).0))
    }

    fn explore_path(&self, unit: &ReviewUnit, instance: &str) -> Result<PathBuf> {
        Self::explore_id(instance)?;
        Ok(self.explore_review(unit)?.join(format!("{instance}.json")))
    }

    fn explore_id(id: &str) -> Result<()> {
        if id.is_empty()
            || id.len() > 128
            || !id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
        {
            return Err(Error::Explore("invalid stored identity".into()));
        }
        Ok(())
    }

    fn explore_lock(&self, unit: &ReviewUnit) -> Result<std::fs::File> {
        let directory = self.explore_review(unit)?;
        self.create_dir(&directory)?;
        let path = directory.join("lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)
            .map_err(|source| Error::StateIo {
                operation: "open Explore lock",
                path: path.clone(),
                source,
            })?;
        file.lock_exclusive().map_err(|source| Error::StateIo {
            operation: "lock Explore",
            path,
            source,
        })?;
        Ok(file)
    }

    pub fn load_explore_history(&self, unit: &ReviewUnit) -> Result<ExploreHistory> {
        Ok(
            Self::read_explore(&self.explore_review(unit)?.join("index.json"), MAX_VIEW)?
                .unwrap_or_default(),
        )
    }

    fn explore_entries(directory: &Path) -> Result<impl Iterator<Item = Result<fs::DirEntry>>> {
        let entries = fs::read_dir(directory).map_err(|source| Error::StateIo {
            operation: "scan Explore state",
            path: directory.to_path_buf(),
            source,
        })?;
        let path = directory.to_path_buf();
        Ok(entries.map(move |entry| {
            entry.map_err(|source| Error::StateIo {
                operation: "scan Explore state",
                path: path.clone(),
                source,
            })
        }))
    }

    fn remove_explore_file(path: &Path) -> Result<()> {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(Error::StateIo {
                operation: "remove Explore state",
                path: path.to_owned(),
                source,
            }),
        }
    }

    pub fn load_explore(&self, unit: &ReviewUnit, instance: &str) -> Result<Option<ExploreRound>> {
        let round: Option<ExploreRound> =
            Self::read_explore(&self.explore_path(unit, instance)?, MAX_DOMAIN)?;
        if let Some(round) = &round {
            if round.exploration.instance != instance
                || &round.exploration.comparison.checkpoint.review_unit != unit
            {
                return Err(Error::Explore(
                    "stored round identity does not match its review".into(),
                ));
            }
            round
                .validate_restored()
                .map_err(|e| Error::Explore(e.to_string()))?;
        }
        Ok(round)
    }

    /// Lock one review's Explore records for a read-modify-write.
    pub fn lock_explore(&self, unit: &ReviewUnit) -> Result<ExploreRecords<'_>> {
        let lock = self.explore_lock(unit)?;
        Ok(ExploreRecords {
            store: self,
            unit: unit.clone(),
            directory: self.explore_review(unit)?,
            _lock: lock,
        })
    }

    fn explore_view_path(&self, unit: &ReviewUnit, instance: &str) -> Result<PathBuf> {
        Self::explore_id(instance)?;
        Ok(self
            .explore_review(unit)?
            .join(format!("{instance}.view.json")))
    }

    fn round_forks_path(&self, unit: &ReviewUnit, instance: &str) -> Result<PathBuf> {
        Self::explore_id(instance)?;
        Ok(self
            .explore_review(unit)?
            .join(format!("{instance}.forks.json")))
    }

    /// The run-ahead forks of the round `instance`; none when it never had any.
    pub fn load_round_forks(&self, unit: &ReviewUnit, instance: &str) -> Result<RoundForks> {
        Ok(
            Self::read_explore(&self.round_forks_path(unit, instance)?, MAX_FORKS)?
                .unwrap_or_default(),
        )
    }

    pub fn load_explore_view(&self, unit: &ReviewUnit, instance: &str) -> Result<Option<ViewSave>> {
        let view: Option<ViewSave> =
            Self::read_explore(&self.explore_view_path(unit, instance)?, MAX_VIEW)?;
        if view
            .as_ref()
            .is_some_and(|view| view.instance != instance || &view.review_unit != unit)
        {
            return Err(Error::Explore("editor belongs to another round".into()));
        }
        Ok(view)
    }

    fn write_explore(&self, path: &Path, value: &impl Serialize, maximum: u64) -> Result<()> {
        let bytes = serde_json::to_vec(&Stored {
            version: VERSION,
            value,
        })
        .map_err(|source| Error::StateJson {
            operation: "encode Explore",
            path: path.to_owned(),
            source,
        })?;
        if bytes.len() as u64 > maximum {
            return Err(Error::Explore(format!(
                "stored record exceeds {maximum} bytes; original retained"
            )));
        }
        self.atomic_write(path, &bytes, "save Explore")
    }

    fn read_explore<T: DeserializeOwned>(path: &Path, maximum: u64) -> Result<Option<T>> {
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
        {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(Error::StateIo {
                    operation: "read Explore",
                    path: path.to_owned(),
                    source,
                });
            }
        };
        let mut bytes = Vec::new();
        file.take(maximum + 1)
            .read_to_end(&mut bytes)
            .map_err(|source| Error::StateIo {
                operation: "read Explore",
                path: path.to_owned(),
                source,
            })?;
        if bytes.len() as u64 > maximum {
            return Err(Error::Explore(format!(
                "oversized data at {}; original retained",
                path.display()
            )));
        }
        let decode_error = |source| Error::StateJson {
            operation: "decode Explore",
            path: path.to_owned(),
            source,
        };
        let version = serde_json::from_slice::<StoredVersion>(&bytes)
            .map_err(decode_error)?
            .version;
        if version < VERSION {
            return Ok(None);
        }
        if version != VERSION {
            return Err(Error::Explore(format!(
                "unsupported version {version} at {}; original retained",
                path.display()
            )));
        }
        let stored: Stored<T> = serde_json::from_slice(&bytes).map_err(decode_error)?;
        Ok(Some(stored.value))
    }
}

#[cfg(test)]
#[path = "explore.tests.rs"]
mod tests;
