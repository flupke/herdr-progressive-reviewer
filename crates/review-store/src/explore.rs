//! Versioned Explore domain transactions and one editor autosave per pass.
use super::{Error, Result, ReviewStore, StateKey};
use fs2::FileExt;
use review_explore::{
    CompletionMark, CoverageFeedback, CoverageReceipt, ExploreHistory, ExplorePass,
    InterviewUpdate, PriorMark, ReviewCompletion, ViewSave,
};
use review_types::ReviewUnit;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    fs::{self, OpenOptions},
    io::Read,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
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

impl ReviewStore {
    /// Serially accept an Explore response and, for a conclusion, recoverably mark its exact files.
    pub fn submit_explore(
        &self,
        unit: &ReviewUnit,
        instance: &str,
        update: &InterviewUpdate,
        exclusions_enabled: bool,
    ) -> Result<(bool, ExplorePass, CoverageReceipt)> {
        let _lock = self.explore_lock(unit)?;
        let history = self.load_explore_history(unit)?;
        if history.is_historical(instance) {
            return Err(Error::Explore(
                "this pass is history; open the latest pass to continue".into(),
            ));
        }
        let mut pass = self
            .load_explore(unit, instance)?
            .ok_or_else(|| Error::Explore("saved pass is missing".into()))?;
        if let Some(completion) = &pass.completion
            && !completion.completed
        {
            let accepted = pass
                .exploration
                .conversation
                .iter()
                .find(|turn| turn.update.request == completion.request)
                .is_some_and(|turn| &turn.update == update);
            if !accepted {
                return Err(Error::Explore(
                    "Explore conclusion finalization is pending; retry the identical payload"
                        .into(),
                ));
            }
            self.finish_explore_marks(unit, instance, &mut pass)?;
            let feedback = pass
                .coverage_receipts
                .get(&update.request)
                .cloned()
                .ok_or_else(|| {
                    Error::Explore("accepted conclusion has no coverage receipt".into())
                })?;
            return Ok((false, pass, feedback));
        }
        let (applied, feedback) = pass
            .submit(update, exclusions_enabled)
            .map_err(|error| Error::Explore(error.to_string()))?;
        if !applied {
            return Ok((false, pass, feedback));
        }
        if update.conclusion.is_some() && pass.completion.is_none() {
            pass.completion = Some(self.prepare_explore_completion(
                unit,
                &pass,
                update,
                exclusions_enabled,
                feedback.feedback(),
            )?);
        }
        self.save_explore_revision(unit, instance, &mut pass)?;
        if pass.completion.is_some() {
            self.finish_explore_marks(unit, instance, &mut pass)?;
        }
        Ok((true, pass, feedback))
    }

    fn prepare_explore_completion(
        &self,
        unit: &ReviewUnit,
        pass: &ExplorePass,
        update: &InterviewUpdate,
        exclusions_enabled: bool,
        feedback: &CoverageFeedback,
    ) -> Result<ReviewCompletion> {
        let comparison = &pass.exploration.comparison;
        if comparison.checkpoint.review_unit != *unit
            || comparison
                .base
                .as_ref()
                .is_none_or(|base| base.snapshot_id() != comparison.checkpoint.checkpoint)
        {
            return Err(Error::Explore(
                "reviewed baseline identity is unavailable".into(),
            ));
        }
        let mut paths = std::collections::BTreeSet::new();
        let mut marks = Vec::new();
        for file in &comparison.files {
            let path = file.review_path().as_bytes();
            if !paths.insert(path.to_vec()) {
                continue;
            }
            let prior = match self.load(unit, path)? {
                super::LoadResult::Unreviewed => None,
                super::LoadResult::Reviewed(record) => Some(PriorMark {
                    baseline: record.baseline_commit_id,
                    reviewed_at: record.reviewed_at,
                }),
                super::LoadResult::UnknownSchema => {
                    return Err(Error::Explore(format!(
                        "cannot replace unknown review mark for {}",
                        file.review_path().display()
                    )));
                }
            };
            marks.push(CompletionMark {
                path: path.to_vec(),
                prior,
                applied: false,
            });
        }
        Ok(ReviewCompletion {
            request: update.request.clone(),
            baseline: comparison.checkpoint.checkpoint.clone(),
            marks,
            completed: false,
            exclusions_enabled,
            summary: feedback.summary.clone(),
            unexplored: Some(review_explore::UnexploredAtConclusion {
                required: pass.coverage.remaining(exclusions_enabled),
                jev_excluded: if exclusions_enabled {
                    pass.coverage.unexplored_exclusions()
                } else {
                    Vec::new()
                },
            }),
        })
    }

    /// Resume only local marks; never dispatch a prompt or replay a completed receipt.
    pub fn recover_explore_marks(&self, unit: &ReviewUnit, instance: &str) -> Result<ExplorePass> {
        let _lock = self.explore_lock(unit)?;
        let mut pass = self
            .load_explore(unit, instance)?
            .ok_or_else(|| Error::Explore("saved pass is missing".into()))?;
        if pass
            .completion
            .as_ref()
            .is_some_and(|record| !record.completed)
        {
            self.finish_explore_marks(unit, instance, &mut pass)?;
        }
        Ok(pass)
    }

    fn finish_explore_marks(
        &self,
        unit: &ReviewUnit,
        instance: &str,
        pass: &mut ExplorePass,
    ) -> Result<()> {
        let Some(completion) = &pass.completion else {
            return Ok(());
        };
        if completion.completed {
            return Ok(());
        }
        let baseline = completion.baseline.clone();
        let count = completion.marks.len();
        for index in 0..count {
            let mark = pass.completion.as_ref().expect("completion").marks[index].clone();
            let current = self.load(unit, &mark.path)?;
            if mark.applied {
                if !matches!(current, super::LoadResult::Reviewed(ref record) if record.baseline_commit_id == baseline)
                {
                    return Err(Error::Explore(
                        "a completed Explore mark was changed during finalization".into(),
                    ));
                }
                continue;
            }
            match current {
                super::LoadResult::Reviewed(ref record)
                    if record.baseline_commit_id == baseline => {}
                super::LoadResult::Unreviewed if mark.prior.is_none() => {
                    self.mark(unit, &mark.path, &baseline)?;
                }
                super::LoadResult::Reviewed(ref record)
                    if mark.prior.as_ref().is_some_and(|prior| {
                        prior.baseline == record.baseline_commit_id
                            && prior.reviewed_at == record.reviewed_at
                    }) =>
                {
                    self.mark(unit, &mark.path, &baseline)?;
                }
                _ => {
                    return Err(Error::Explore(format!(
                        "review mark changed while Explore was finalizing: {}",
                        String::from_utf8_lossy(&mark.path)
                    )));
                }
            }
            pass.completion.as_mut().expect("completion").marks[index].applied = true;
            self.save_explore_revision(unit, instance, pass)?;
        }
        pass.completion.as_mut().expect("completion").completed = true;
        self.save_explore_revision(unit, instance, pass)
    }

    fn save_explore_revision(
        &self,
        unit: &ReviewUnit,
        instance: &str,
        pass: &mut ExplorePass,
    ) -> Result<()> {
        pass.revision = pass
            .revision
            .checked_add(1)
            .ok_or_else(|| Error::Explore("revision exhausted".into()))?;
        self.write_explore(&self.explore_path(unit, instance)?, pass, MAX_DOMAIN)
    }
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

    /// Discard this review's unreadable Explore history without touching review marks or threads.
    pub fn clear_explore(&self, unit: &ReviewUnit) -> Result<()> {
        let _lock = self.explore_lock(unit)?;
        let directory = self.explore_review(unit)?;
        let index = directory.join("index.json");
        Self::remove_explore_file(&index)?;
        for entry in Self::explore_entries(&directory)? {
            let entry = entry?;
            if entry.file_name() == "lock" {
                continue;
            }
            Self::remove_explore_file(&entry.path())?;
        }
        self.sync_parent(&index)
    }

    /// Rebuild a damaged index from readable passes, retaining their editor snapshots.
    pub fn repair_explore_history(&self, unit: &ReviewUnit) -> Result<ExploreHistory> {
        let _lock = self.explore_lock(unit)?;
        let directory = self.explore_review(unit)?;
        let mut passes = Vec::new();
        for entry in Self::explore_entries(&directory)? {
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
            if self.load_explore(unit, instance).ok().flatten().is_some() {
                let modified = entry
                    .metadata()
                    .and_then(|metadata| metadata.modified())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                passes.push((modified, instance.to_owned()));
            }
        }
        passes.sort();
        let history = ExploreHistory {
            passes: passes.into_iter().map(|(_, instance)| instance).collect(),
            latest_editable: false,
        };
        self.write_explore(&directory.join("index.json"), &history, MAX_VIEW)?;
        Ok(history)
    }

    /// Remove one unreadable pass and its editor state while preserving other passes.
    pub fn clear_explore_pass(&self, unit: &ReviewUnit, instance: &str) -> Result<()> {
        let _lock = self.explore_lock(unit)?;
        let mut history = self.load_explore_history(unit)?;
        if history.passes.last().is_some_and(|saved| saved == instance) {
            history.latest_editable = false;
        }
        history.passes.retain(|saved| saved != instance);
        let index = self.explore_review(unit)?.join("index.json");
        self.write_explore(&index, &history, MAX_VIEW)?;
        Self::remove_explore_file(&self.explore_path(unit, instance)?)?;
        Self::remove_explore_file(&self.explore_view_path(unit, instance)?)?;
        self.sync_parent(&index)
    }

    /// Discard a damaged editor snapshot while retaining its valid interview.
    pub fn clear_explore_view(&self, unit: &ReviewUnit, instance: &str) -> Result<()> {
        let _lock = self.explore_lock(unit)?;
        let path = self.explore_view_path(unit, instance)?;
        Self::remove_explore_file(&path)?;
        self.sync_parent(&path)
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
        history.latest_editable = true;
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
        if history.is_historical(instance) {
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
