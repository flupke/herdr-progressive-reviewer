//! Brings the review state that each jj workspace or Git worktree kept for itself into the state
//! its repository's workspaces share (ADR 0013).
//!
//! Each open of a reviewer finds the per-workspace states of its repository still at the root
//! of the state directory: those of its current workspaces by path, and every other one by the
//! reviews it holds, which the repository must hold too, hidden jj changes included. Before the
//! first merge, every per-workspace state is backed up beside the state root; then each state of
//! the repository is merged into the shared state and moved to `merged-workspaces/`.

mod backup;

use std::collections::BTreeSet;
use std::path::PathBuf;

use review_repository::repository::Repository;
use review_store::{ReviewStore, WorkspaceState};

/// What a migration did.
#[derive(Debug, Default)]
pub struct Migration {
    /// Where each merged per-workspace state moved.
    pub merged: Vec<PathBuf>,
    /// The records of merged states that could not be read: they stay in the moved state, and
    /// in the backup, only.
    pub unreadable: Vec<PathBuf>,
}

/// Why a migration stopped.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The backup failed, so nothing was merged.
    #[error("Could not back up the review state; nothing was merged: {0}")]
    Backup(String),
    /// The merge stopped; the states it had not merged yet stay where they were.
    #[error("Could not merge the review state of this repository's other workspaces: {0}")]
    Merge(String),
}

/// Merge the per-workspace states of `repository` into `store`, its shared state, after a
/// backup. It holds the lock of every migration of the state root throughout, so a reviewer
/// that opens at the same time waits, then finds nothing left to merge. Merging a state that a
/// reviewer of an earlier build wrote again after it was merged first backs it up again.
pub fn migrate(repository: &Repository, store: &ReviewStore) -> Result<Migration, Error> {
    let merge = |error: &dyn std::fmt::Display| Error::Merge(error.to_string());
    let _lock = store.lock_migration().map_err(|error| merge(&error))?;
    let states = WorkspaceState::list(store.state_root()).map_err(|error| merge(&error))?;
    if states.is_empty() {
        return Ok(Migration::default());
    }
    let found = repository_states(repository, store, &states).map_err(|error| merge(&*error))?;
    if !found.ours.is_empty() {
        backup::before_merge(store, &states, &found.ours)
            .map_err(|error| Error::Backup(error.to_string()))?;
    }
    // Written once the backup holds every state it read: a failed backup writes nothing.
    if let Some(foreign) = found.foreign {
        store
            .save_foreign_workspace_states(&foreign)
            .map_err(|error| merge(&error))?;
    }
    let mut migration = Migration::default();
    for state in found.ours {
        let unreadable = store
            .merge_workspace_state(&state)
            .map_err(|error| merge(&error))?;
        migration.unreadable.extend(unreadable);
        migration.merged.push(
            store
                .retire_workspace_state(&state)
                .map_err(|error| merge(&error))?,
        );
    }
    Ok(migration)
}

/// Which per-workspace states belong to a repository.
struct Found {
    ours: Vec<WorkspaceState>,
    /// Every state found to be another repository's, when this check found new ones.
    foreign: Option<BTreeSet<String>>,
}

/// The states among `states` that belong to `repository`: those of its current workspaces, by
/// path, and those that hold one of its reviews. Those that hold only reviews of other
/// repositories are another's, to record so the next open does not ask the repository about
/// them again. A state that holds no review belongs to none, and is checked again each time.
fn repository_states(
    repository: &Repository,
    store: &ReviewStore,
    states: &[WorkspaceState],
) -> Result<Found, Box<dyn std::error::Error>> {
    let current = repository
        .workspace_roots()?
        .iter()
        .map(|root| WorkspaceState::key_of(root))
        .collect::<Result<BTreeSet<_>, _>>()?;
    let known = store.foreign_workspace_states()?;
    let mut foreign = known.clone();
    let mut ours = Vec::new();
    for state in states {
        if current.contains(state.key()) {
            ours.push(state.clone());
        } else if !foreign.contains(state.key()) {
            let units = state.review_units()?;
            if units.is_empty() {
                continue;
            }
            if repository.holds_any_review_unit(&units)? {
                ours.push(state.clone());
            } else {
                foreign.insert(state.key().to_owned());
            }
        }
    }
    Ok(Found {
        ours,
        foreign: (foreign != known).then_some(foreign),
    })
}

#[cfg(test)]
#[path = "lib.tests.rs"]
mod tests;
