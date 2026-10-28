//! The review state that each jj workspace or Git worktree kept for itself before the
//! workspaces of a repository shared one (ADR 0013), and its merge into the shared state.
//!
//! Such a state is a folder at the root of the state directory, named by the SHA-256 of the
//! workspace's canonical root. The merge reads it and never writes it; once merged, the folder
//! moves to `merged-workspaces/`.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use review_explore::ExploreHistory;
use review_types::ReviewUnit;
use serde::Deserialize;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::explore::MAX_VIEW;
use super::{Error, Result, ReviewStore, StateKey};

/// Where merged per-workspace states move, under the state root.
const MERGED_WORKSPACES: &str = "merged-workspaces";
/// The file in a moved state that names the shared folder it was merged into.
const MERGED_INTO: &str = "merged-into";
/// The file in a repository's folder that lists the per-workspace states found to belong to
/// another repository, so each is checked once.
const FOREIGN_WORKSPACES: &str = "foreign-workspaces.json";

/// The review state one workspace kept for itself.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceState {
    key: String,
    path: PathBuf,
}

/// A mark file as the merge compares it: by when it was written.
#[derive(Deserialize)]
struct MarkTime {
    reviewed_at: String,
}

/// The review of a stored thread document.
#[derive(Deserialize)]
struct StoredThreadsOwner {
    conversations: StoredOwner,
}

/// The review a stored document belongs to.
#[derive(Deserialize)]
struct StoredOwner {
    review_unit: ReviewUnit,
}

/// The review of a stored Explore round, read without the rest of the round.
#[derive(Deserialize)]
struct StoredRoundOwner {
    value: RoundOwner,
}

#[derive(Deserialize)]
struct RoundOwner {
    exploration: ExplorationOwner,
}

#[derive(Deserialize)]
struct ExplorationOwner {
    comparison: ComparisonOwner,
}

#[derive(Deserialize)]
struct ComparisonOwner {
    checkpoint: StoredOwner,
}

impl WorkspaceState {
    /// Every per-workspace state left at the root of `state_root`: the folders named by 64
    /// lowercase hex digits.
    pub fn list(state_root: &Path) -> Result<Vec<Self>> {
        let entries = match fs::read_dir(state_root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => return Err(io("scan review state", state_root)(source)),
        };
        let mut states = Vec::new();
        for entry in entries {
            let entry = entry.map_err(io("scan review state", state_root))?;
            let Some(key) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if Self::is_key(&key) && entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                states.push(Self {
                    key,
                    path: entry.path(),
                });
            }
        }
        states.sort_by(|left, right| left.key.cmp(&right.key));
        Ok(states)
    }

    /// The key a workspace whose canonical root is `root` kept its state under.
    pub fn key_of(root: &Path) -> Result<String> {
        let root = root.to_str().ok_or(Error::InvalidStateKey {
            field: "workspace root",
        })?;
        Ok(StateKey::hash(root.as_bytes()).0)
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn is_key(name: &str) -> bool {
        name.len() == 64
            && name
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }

    /// Every review the state holds a record of: the reviews of its marks, threads, drafts
    /// and Explore rounds. A record it cannot read names none.
    pub fn review_units(&self) -> Result<BTreeSet<ReviewUnit>> {
        let mut units = BTreeSet::new();
        for (name, _) in entries(&self.path.join("changes"))? {
            if name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
            {
                units.insert(ReviewUnit::from(name));
            }
        }
        for kind in ["conversations", "drafts"] {
            for (_, path) in entries(&self.path.join(kind))? {
                if let Ok(unit) = record_unit(kind, &path) {
                    units.insert(unit);
                }
            }
        }
        for (_, review) in entries(&self.path.join("explore-v1"))? {
            if let Some(unit) = Self::explore_unit(&review) {
                units.insert(unit);
            }
        }
        units.retain(|unit| !unit.is_empty());
        Ok(units)
    }

    /// The review of the first readable round in the Explore folder `review`.
    fn explore_unit(review: &Path) -> Option<ReviewUnit> {
        ReviewStore::round_files_in(review)
            .unwrap_or_default()
            .into_iter()
            .find_map(|(instance, _)| {
                let bytes = fs::read(review.join(format!("{instance}.json"))).ok()?;
                let round = serde_json::from_slice::<StoredRoundOwner>(&bytes).ok()?;
                Some(round.value.exploration.comparison.checkpoint.review_unit)
            })
    }

    /// When anything in the state last changed: the latest modification time of its folders
    /// and files.
    pub fn last_modified(&self) -> Result<SystemTime> {
        latest_modification(&self.path)
    }
}

impl ReviewStore {
    /// The folder of this repository's state.
    pub fn directory(&self) -> &Path {
        &self.repository_dir
    }

    /// The root of the state directory, which every repository's state shares.
    pub fn state_root(&self) -> &Path {
        &self.state_root
    }

    /// Lock the migration of per-workspace states for every reviewer process of the state root,
    /// until the returned file is dropped.
    pub fn lock_migration(&self) -> Result<fs::File> {
        self.exclusive_lock(&self.state_root.join("migration.lock"), "lock migration")
    }

    /// The per-workspace states this repository checked and found to be another's.
    pub fn foreign_workspace_states(&self) -> Result<BTreeSet<String>> {
        Ok(Self::read_json(
            &self.repository_dir.join(FOREIGN_WORKSPACES),
            "read foreign workspace states",
        )?
        .unwrap_or_default())
    }

    /// Record the per-workspace states this repository checked and found to be another's.
    pub fn save_foreign_workspace_states(&self, keys: &BTreeSet<String>) -> Result<()> {
        self.atomic_json(
            &self.repository_dir.join(FOREIGN_WORKSPACES),
            keys,
            "write foreign workspace states",
        )
    }

    /// Merge every record of `state`, a per-workspace state of this repository, into the shared
    /// state, each under the lock its own writes take, and return the records that could not be
    /// read: they stay in `state` only. Merging the same state again changes nothing.
    ///
    /// A review mark goes in when the shared state has none for its file, or an older one. The
    /// threads and drafts of a review are merged by thread. Explore rounds and their views and
    /// run-ahead records are copied; when both states hold a history of the same review, the
    /// merged history lists the rounds of both in the order they were saved, none of them
    /// editable, unless the shared history already lists every round of the other.
    pub fn merge_workspace_state(&self, state: &WorkspaceState) -> Result<Vec<PathBuf>> {
        let old = Self {
            state_root: self.state_root.clone(),
            repository_dir: state.path.clone(),
            thread_sources: std::sync::Arc::default(),
        };
        let mut unreadable = Vec::new();
        self.merge_thread_contexts(&old)?;
        self.merge_marks(&old, &mut unreadable)?;
        self.merge_threads(&old, &mut unreadable)?;
        self.merge_drafts(&old, &mut unreadable)?;
        self.merge_explore(&old, &mut unreadable)?;
        Ok(unreadable)
    }

    /// Move a merged per-workspace state to `merged-workspaces/`, with a file that names the
    /// shared folder it was merged into, and return where it moved.
    pub fn retire_workspace_state(&self, state: &WorkspaceState) -> Result<PathBuf> {
        let merged = self.state_root.join(MERGED_WORKSPACES);
        self.create_dir(&merged)?;
        let mut target = merged.join(&state.key);
        let mut attempt = 1;
        while target.exists() {
            attempt += 1;
            target = merged.join(format!("{}-{attempt}", state.key));
        }
        fs::rename(&state.path, &target).map_err(io("move merged workspace state", &target))?;
        self.sync_parent(&target)?;
        let shared = self
            .repository_dir
            .strip_prefix(&self.state_root)
            .unwrap_or(&self.repository_dir);
        let note = format!("{}\n", shared.display());
        self.atomic_write(
            &target.join(MERGED_INTO),
            note.as_bytes(),
            "name merged state",
        )?;
        Ok(target)
    }

    /// Copy the thread contexts the shared state lacks; they are named by their content.
    fn merge_thread_contexts(&self, old: &Self) -> Result<()> {
        for (name, path) in entries(&old.repository_dir.join("thread-contexts"))? {
            let target = self.repository_dir.join("thread-contexts").join(&name);
            if !name.starts_with(".tmp-") && !target.exists() {
                let bytes = fs::read(&path).map_err(io("read thread context", &path))?;
                self.atomic_write(&target, &bytes, "copy thread context")?;
            }
        }
        Ok(())
    }

    fn merge_marks(&self, old: &Self, unreadable: &mut Vec<PathBuf>) -> Result<()> {
        for (unit, _) in entries(&old.repository_dir.join("changes"))? {
            let unit = ReviewUnit::from(unit);
            if super::checkpoint::ReviewUnitKey::validate(&unit).is_err() {
                unreadable.push(old.record_directory(&unit));
                continue;
            }
            let _lock = self.lock_marks(&unit)?;
            let mut changed = false;
            for (name, path) in entries(&old.record_directory(&unit))? {
                if name.starts_with(".tmp-") {
                    continue;
                }
                let bytes = fs::read(&path).map_err(io("read review mark", &path))?;
                let Ok(mark) = serde_json::from_slice::<MarkTime>(&bytes) else {
                    unreadable.push(path);
                    continue;
                };
                let target = self.record_directory(&unit).join(&name);
                let kept = fs::read(&target)
                    .ok()
                    .and_then(|kept| serde_json::from_slice::<MarkTime>(&kept).ok());
                if kept.is_none_or(|kept| later(&mark.reviewed_at, &kept.reviewed_at)) {
                    self.atomic_write(&target, &bytes, "merge review mark")?;
                    changed = true;
                }
            }
            if changed {
                self.stamp_marks(&unit)?;
            }
        }
        Ok(())
    }

    fn merge_threads(&self, old: &Self, unreadable: &mut Vec<PathBuf>) -> Result<()> {
        for (_, path) in entries(&old.repository_dir.join("conversations"))? {
            if path.extension().is_none_or(|extension| extension != "zst") {
                continue;
            }
            let document = record_unit("conversations", &path).and_then(|unit| {
                old.read_thread_document(&unit)
                    .map(|document| (unit, document))
            });
            let Ok((unit, document)) = document else {
                unreadable.push(path);
                continue;
            };
            self.update_threads(&unit, |threads| threads.merge(document.threads))?;
            if !document.embedded_drafts.is_empty() {
                self.update_drafts(&unit, |drafts, _| {
                    drafts.merge(document.embedded_drafts);
                    Ok(())
                })?;
            }
        }
        Ok(())
    }

    fn merge_drafts(&self, old: &Self, unreadable: &mut Vec<PathBuf>) -> Result<()> {
        for (_, path) in entries(&old.repository_dir.join("drafts"))? {
            if path.extension().is_none_or(|extension| extension != "zst") {
                continue;
            }
            let drafts = record_unit("drafts", &path)
                .and_then(|unit| old.read_drafts(&unit).map(|drafts| (unit, drafts)));
            let Ok((unit, drafts)) = drafts else {
                unreadable.push(path);
                continue;
            };
            self.update_drafts(&unit, |kept, _| {
                kept.merge(drafts);
                Ok(())
            })?;
        }
        Ok(())
    }

    fn merge_explore(&self, old: &Self, unreadable: &mut Vec<PathBuf>) -> Result<()> {
        for (name, from) in entries(&old.explore_directory())? {
            if !from.is_dir() {
                continue;
            }
            let to = self.explore_directory().join(&name);
            let _lock = self.lock_explore_review(&to)?;
            for (file, path) in entries(&from)? {
                let record = path
                    .extension()
                    .is_some_and(|extension| extension == "json");
                if record && file != "index.json" && !file.starts_with(".tmp-") {
                    self.copy_newer(&path, &to.join(&file))?;
                }
            }
            let index = from.join("index.json");
            let history = if let Ok(history) = Self::read_explore(&index, MAX_VIEW) {
                history
            } else {
                unreadable.push(index);
                Some(Self::rounds_in_saved_order(&to)?)
            };
            if let Some(history) = history {
                self.merge_history(&to, history)?;
            }
        }
        Ok(())
    }

    /// Merge `old`, the history of one review in a per-workspace state, into the shared
    /// history in `directory`, whose round files the shared state now holds.
    fn merge_history(&self, directory: &Path, old: ExploreHistory) -> Result<()> {
        let path = directory.join("index.json");
        let Ok(kept) = Self::read_explore::<ExploreHistory>(&path, MAX_VIEW) else {
            // Restoring the review rebuilds a damaged history from the round files.
            return Ok(());
        };
        let merged = match kept {
            None => old,
            Some(kept) if old.rounds.iter().all(|round| kept.rounds.contains(round)) => {
                return Ok(());
            }
            Some(_) => Self::rounds_in_saved_order(directory)?,
        };
        self.write_explore(&path, &merged, MAX_VIEW)
    }

    /// The history of every round file in `directory`, oldest first, none of them editable.
    fn rounds_in_saved_order(directory: &Path) -> Result<ExploreHistory> {
        let mut rounds = Self::round_files_in(directory)?
            .into_iter()
            .map(|(instance, saved)| (saved, instance))
            .collect::<Vec<_>>();
        rounds.sort();
        Ok(ExploreHistory {
            rounds: rounds.into_iter().map(|(_, instance)| instance).collect(),
            latest_editable: false,
            ..ExploreHistory::default()
        })
    }

    /// Copy `from` to `to` when `to` is missing or older, keeping the time `from` was saved.
    fn copy_newer(&self, from: &Path, to: &Path) -> Result<()> {
        let saved = modified(from)?;
        if to.exists() && modified(to)? >= saved {
            return Ok(());
        }
        let bytes = fs::read(from).map_err(io("read Explore record", from))?;
        self.atomic_write(to, &bytes, "copy Explore record")?;
        fs::File::options()
            .write(true)
            .open(to)
            .and_then(|file| file.set_modified(saved))
            .map_err(io("keep Explore record time", to))
    }
}

/// The review a stored thread document (`conversations`) or draft document (`drafts`) names.
fn record_unit(kind: &str, path: &Path) -> Result<ReviewUnit> {
    let bytes = fs::read(path).map_err(io("read review record", path))?;
    let json = ReviewStore::decode_thread_json(path, &bytes)?;
    let decode = Error::json("decode review record", path);
    Ok(if kind == "conversations" {
        serde_json::from_slice::<StoredThreadsOwner>(&json)
            .map_err(decode)?
            .conversations
            .review_unit
    } else {
        serde_json::from_slice::<StoredOwner>(&json)
            .map_err(decode)?
            .review_unit
    })
}

/// Whether the mark written at `time` is later than the one written at `than`. Times this
/// reviewer cannot read compare as text.
fn later(time: &str, than: &str) -> bool {
    match (
        OffsetDateTime::parse(time, &Rfc3339),
        OffsetDateTime::parse(than, &Rfc3339),
    ) {
        (Ok(time), Ok(than)) => time > than,
        _ => time > than,
    }
}

/// The name and path of each entry of `directory`; none when it is missing.
fn entries(directory: &Path) -> Result<Vec<(String, PathBuf)>> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(io("scan workspace state", directory)(source)),
    };
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry.map_err(io("scan workspace state", directory))?;
        if let Some(name) = entry.file_name().to_str() {
            found.push((name.to_owned(), entry.path()));
        }
    }
    found.sort();
    Ok(found)
}

fn latest_modification(path: &Path) -> Result<SystemTime> {
    let mut latest = modified(path)?;
    if fs::symlink_metadata(path)
        .map_err(io("inspect workspace state", path))?
        .is_dir()
    {
        for (_, entry) in entries(path)? {
            latest = latest.max(latest_modification(&entry)?);
        }
    }
    Ok(latest)
}

fn modified(path: &Path) -> Result<SystemTime> {
    fs::symlink_metadata(path)
        .and_then(|metadata| metadata.modified())
        .map_err(io("inspect workspace state", path))
}

fn io(operation: &'static str, path: &Path) -> impl Fn(std::io::Error) -> Error {
    let path = path.to_owned();
    move |source| Error::StateIo {
        operation,
        path: path.clone(),
        source,
    }
}

#[cfg(test)]
#[path = "workspace_state.tests.rs"]
mod tests;
