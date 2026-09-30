//! The Explore record format and exclusive, locked access to one review's records.
//!
//! Each review keeps a history index, one file per pass and one editor view per pass.
//! The store only reads, writes and removes them; what a change means belongs to the
//! Explore session.
use super::{Error, Result, ReviewStore, StateKey};
use fs2::FileExt;
use review_explore::{ExploreHistory, ExplorePass, ViewSave};
use review_types::ReviewUnit;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    fs::{self, OpenOptions},
    io::Read,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    time::SystemTime,
};

const VERSION: u32 = 1;
// A pass grows across many valid 1 MiB submissions. This is not a source archive.
const MAX_DOMAIN: u64 = 256 * 1024 * 1024;
const MAX_VIEW: u64 = 16 * 1024 * 1024;

#[derive(Deserialize, Serialize)]
struct Stored<T> {
    version: u32,
    value: T,
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

    pub fn pass(&self, instance: &str) -> Result<Option<ExplorePass>> {
        self.store.load_explore(&self.unit, instance)
    }

    /// Save a pass that has no record yet. A saved pass only changes through
    /// [`Self::update_pass`], so no caller writes a stale whole-pass copy.
    pub fn create_pass(&self, pass: &ExplorePass) -> Result<()> {
        if self.pass(&pass.exploration.instance)?.is_some() {
            return Err(Error::Explore("pass already exists".into()));
        }
        self.save_pass(pass)
    }

    fn save_pass(&self, pass: &ExplorePass) -> Result<()> {
        if pass.exploration.comparison.checkpoint.review_unit != self.unit {
            return Err(Error::Explore("pass belongs to another review".into()));
        }
        self.store.write_explore(
            &self
                .store
                .explore_path(&self.unit, &pass.exploration.instance)?,
            pass,
            MAX_DOMAIN,
        )
    }

    /// Change a saved pass, writing it with the next revision only when it changed.
    pub fn update_pass<T>(
        &self,
        instance: &str,
        update: impl FnOnce(&mut ExplorePass) -> std::result::Result<T, String>,
    ) -> Result<(T, ExplorePass)> {
        let mut pass = self
            .pass(instance)?
            .ok_or_else(|| Error::Explore("saved pass is missing".into()))?;
        let original = pass.clone();
        let result = update(&mut pass).map_err(Error::Explore)?;
        if pass != original {
            pass.revision = pass
                .revision
                .checked_add(1)
                .ok_or_else(|| Error::Explore("revision exhausted".into()))?;
            self.save_pass(&pass)?;
        }
        Ok((result, pass))
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

    /// Remove a pass file; a missing file is already removed.
    pub fn remove_pass(&self, instance: &str) -> Result<()> {
        ReviewStore::remove_explore_file(&self.store.explore_path(&self.unit, instance)?)
    }

    /// Remove an editor view file; a missing file is already removed.
    pub fn remove_view(&self, instance: &str) -> Result<()> {
        ReviewStore::remove_explore_file(&self.store.explore_view_path(&self.unit, instance)?)
    }

    /// Make earlier removals durable.
    pub fn sync(&self) -> Result<()> {
        self.store.sync_parent(&self.directory.join("index.json"))
    }

    /// The identity and modification time of every pass file, readable or not.
    pub fn pass_files(&self) -> Result<Vec<(String, SystemTime)>> {
        let mut passes = Vec::new();
        for entry in ReviewStore::explore_entries(&self.directory)? {
            let entry = entry?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Some(instance) = name.strip_suffix(".json") else {
                continue;
            };
            if instance == "index" || ReviewStore::explore_id(instance).is_err() {
                continue;
            }
            let modified = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            passes.push((instance.to_owned(), modified));
        }
        Ok(passes)
    }
}

impl ReviewStore {
    /// Watch this namespace with the existing filesystem event infrastructure.
    pub fn explore_directory(&self) -> PathBuf {
        self.repository_dir.join("explore-v1")
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

    pub fn load_explore(&self, unit: &ReviewUnit, instance: &str) -> Result<Option<ExplorePass>> {
        let pass: Option<ExplorePass> =
            Self::read_explore(&self.explore_path(unit, instance)?, MAX_DOMAIN)?;
        if let Some(pass) = &pass {
            if pass.exploration.instance != instance
                || &pass.exploration.comparison.checkpoint.review_unit != unit
            {
                return Err(Error::Explore(
                    "stored pass identity does not match its review".into(),
                ));
            }
            pass.validate_restored()
                .map_err(|e| Error::Explore(e.to_string()))?;
        }
        Ok(pass)
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

    pub fn load_explore_view(&self, unit: &ReviewUnit, instance: &str) -> Result<Option<ViewSave>> {
        let view: Option<ViewSave> =
            Self::read_explore(&self.explore_view_path(unit, instance)?, MAX_VIEW)?;
        if view
            .as_ref()
            .is_some_and(|view| view.instance != instance || &view.review_unit != unit)
        {
            return Err(Error::Explore("editor belongs to another pass".into()));
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
        let stored: Stored<T> =
            serde_json::from_slice(&bytes).map_err(|source| Error::StateJson {
                operation: "decode Explore",
                path: path.to_owned(),
                source,
            })?;
        if stored.version != VERSION {
            return Err(Error::Explore(format!(
                "unsupported version {} at {}; original retained",
                stored.version,
                path.display()
            )));
        }
        Ok(Some(stored.value))
    }
}

#[cfg(test)]
#[path = "explore.tests.rs"]
mod tests;
