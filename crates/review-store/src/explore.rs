//! Versioned Explore domain transactions and one editor autosave per pass.
use super::{Error, Result, ReviewStore, StateKey};
use fs2::FileExt;
use review_explore::{ExplorePass, ViewSave};
use review_types::ReviewUnit;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    fs::OpenOptions,
    io::Read,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

const VERSION: u32 = 1;
// A pass grows across many valid 1 MiB submissions. This is not a source archive.
const MAX_DOMAIN: u64 = 256 * 1024 * 1024;
const MAX_VIEW: u64 = 16 * 1024 * 1024;

#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
pub struct ExploreHistory {
    pub passes: Vec<String>,
}

#[derive(Deserialize, Serialize)]
struct Stored<T> {
    version: u32,
    value: T,
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

    pub fn create_explore(&self, pass: ExplorePass) -> Result<ExplorePass> {
        let unit = &pass.exploration.comparison.checkpoint.review_unit;
        let _lock = self.explore_lock(unit)?;
        let mut history = self.load_explore_history(unit)?;
        let instance = &pass.exploration.instance;
        if self.load_explore(unit, instance)?.is_some() {
            return Err(Error::Explore("pass already exists".into()));
        }
        self.write_explore(&self.explore_path(unit, instance)?, &pass, MAX_DOMAIN)?;
        history.passes.push(instance.clone());
        self.write_explore(
            &self.explore_review(unit)?.join("index.json"),
            &history,
            MAX_VIEW,
        )?;
        Ok(pass)
    }

    /// Read-modify-write under the shared lock; callers never save stale whole-pass copies.
    pub fn update_explore<T>(
        &self,
        unit: &ReviewUnit,
        instance: &str,
        update: impl FnOnce(&mut ExplorePass) -> std::result::Result<T, String>,
    ) -> Result<(T, ExplorePass)> {
        let _lock = self.explore_lock(unit)?;
        let history = self.load_explore_history(unit)?;
        if history.passes.last().map(String::as_str) != Some(instance) {
            return Err(Error::Explore(
                "this pass is history; open the latest pass to continue".into(),
            ));
        }
        self.mutate_explore(unit, instance, update)
    }

    /// A started external call may complete after New pass. Only its result can change history.
    pub fn finish_explore_dispatch(
        &self,
        unit: &ReviewUnit,
        instance: &str,
        result: &review_explore::DispatchResult,
    ) -> Result<ExplorePass> {
        let _lock = self.explore_lock(unit)?;
        if !self
            .load_explore_history(unit)?
            .passes
            .iter()
            .any(|id| id == instance)
        {
            return Err(Error::Explore("saved pass is missing".into()));
        }
        self.mutate_explore(unit, instance, |pass| {
            pass.finish_dispatch(result);
            Ok(())
        })
        .map(|((), pass)| pass)
    }

    // Caller holds the per-review lock. No public whole-pass writes.
    fn mutate_explore<T>(
        &self,
        unit: &ReviewUnit,
        instance: &str,
        update: impl FnOnce(&mut ExplorePass) -> std::result::Result<T, String>,
    ) -> Result<(T, ExplorePass)> {
        let mut pass = self
            .load_explore(unit, instance)?
            .ok_or_else(|| Error::Explore("saved pass is missing".into()))?;
        let original = pass.clone();
        let result = update(&mut pass).map_err(Error::Explore)?;
        if pass != original {
            pass.revision = pass
                .revision
                .checked_add(1)
                .ok_or_else(|| Error::Explore("revision exhausted".into()))?;
            self.write_explore(&self.explore_path(unit, instance)?, &pass, MAX_DOMAIN)?;
        }
        Ok((result, pass))
    }

    /// Save the single reviewer's editor state without rewriting domain history.
    pub fn save_explore_view(&self, unit: &ReviewUnit, view: &ViewSave) -> Result<()> {
        if &view.review_unit != unit {
            return Err(Error::Explore("editor belongs to another review".into()));
        }
        let _lock = self.explore_lock(unit)?;
        // Editor records never rewrite domain history; avoid rereading a long pass per keystroke.
        let history = self.load_explore_history(unit)?;
        if !history.passes.contains(&view.instance) {
            return Err(Error::Explore("saved pass is missing".into()));
        }
        let path = self.explore_view_path(unit, &view.instance)?;
        if let Some(previous) = Self::read_explore::<ViewSave>(&path, MAX_VIEW)? {
            if previous.sequence == view.sequence && previous != *view {
                return Err(Error::Explore(
                    "editor sequence already has different content; original retained".into(),
                ));
            }
            if previous.sequence >= view.sequence {
                return Ok(());
            }
        }
        self.write_explore(&path, view, MAX_VIEW)
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
