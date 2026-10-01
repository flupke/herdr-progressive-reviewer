//! What each durable Explore change means: which pass may change, when a conclusion
//! is final, how delivery outcomes are recorded and how damaged records are repaired.
//!
//! The store owns the record format and the per-review lock; every rule here runs
//! while that lock is held.

use review_explore::{
    DispatchResult, ExploreHistory, ExplorePass, InterviewUpdate, ReviewCompletion, ViewSave,
};
use review_store::{Error, Result, ReviewStore};
use review_types::ReviewUnit;

fn explore_error(reason: &str) -> Error {
    Error::Explore(reason.into())
}

fn historical_pass_error() -> Error {
    explore_error("this pass is history; open the latest pass to continue")
}

/// A pass after an Explore response was submitted to it.
pub(crate) struct Submitted {
    /// False when the response was already accepted and changed nothing.
    pub(crate) applied: bool,
    pub(crate) pass: ExplorePass,
}

/// The saved Explore passes of every review, changed only through the session's rules.
#[derive(Clone)]
pub(crate) struct SavedPasses {
    store: ReviewStore,
}

impl SavedPasses {
    pub(crate) fn new(store: ReviewStore) -> Self {
        Self { store }
    }

    pub(crate) fn history(&self, unit: &ReviewUnit) -> Result<ExploreHistory> {
        self.store.load_explore_history(unit)
    }

    pub(crate) fn pass(&self, unit: &ReviewUnit, instance: &str) -> Result<Option<ExplorePass>> {
        self.store.load_explore(unit, instance)
    }

    pub(crate) fn view(&self, unit: &ReviewUnit, instance: &str) -> Result<Option<ViewSave>> {
        self.store.load_explore_view(unit, instance)
    }

    /// Save a new pass, then make it the editable latest pass of its review.
    pub(crate) fn create(&self, pass: ExplorePass) -> Result<ExplorePass> {
        let records = self
            .store
            .lock_explore(&pass.exploration.comparison.checkpoint.review_unit)?;
        let mut history = records.history()?;
        records.create_pass(&pass)?;
        history.passes.push(pass.exploration.instance.clone());
        history.latest_editable = true;
        records.save_history(&history)?;
        Ok(pass)
    }

    /// Change the editable latest pass; earlier passes are history.
    pub(crate) fn update<T>(
        &self,
        unit: &ReviewUnit,
        instance: &str,
        update: impl FnOnce(&mut ExplorePass) -> std::result::Result<T, String>,
    ) -> Result<(T, ExplorePass)> {
        let records = self.store.lock_explore(unit)?;
        if records.history()?.is_historical(instance) {
            return Err(historical_pass_error());
        }
        records.update_pass(instance, update)
    }

    /// Serially accept an Explore response without changing file review marks.
    pub(crate) fn submit(
        &self,
        unit: &ReviewUnit,
        instance: &str,
        update: &InterviewUpdate,
    ) -> Result<Submitted> {
        let records = self.store.lock_explore(unit)?;
        if records.history()?.is_historical(instance) {
            return Err(historical_pass_error());
        }
        let (applied, pass) = records.update_pass(instance, |pass| {
            let applied = pass.submit(update).map_err(|error| error.to_string())?;
            if applied && update.conclusion.is_some() && pass.completion.is_none() {
                pass.completion = Some(ReviewCompletion {
                    request: update.request.clone(),
                    baseline: pass.exploration.comparison.checkpoint.checkpoint.clone(),
                });
            }
            Ok(applied)
        })?;
        Ok(Submitted { applied, pass })
    }

    /// A started external call may complete after New pass. Only its result can change history.
    pub(crate) fn finish_dispatch(
        &self,
        unit: &ReviewUnit,
        instance: &str,
        result: &DispatchResult,
    ) -> Result<ExplorePass> {
        let records = self.store.lock_explore(unit)?;
        if !records.history()?.passes.iter().any(|id| id == instance) {
            return Err(explore_error("saved pass is missing"));
        }
        records
            .update_pass(instance, |pass| {
                pass.finish_dispatch(result);
                Ok(())
            })
            .map(|((), pass)| pass)
    }

    /// Rebuild a damaged index from readable passes, oldest first, retaining their
    /// editor views. A rebuilt history never guesses which pass is editable.
    pub(crate) fn repair_history(&self, unit: &ReviewUnit) -> Result<ExploreHistory> {
        let records = self.store.lock_explore(unit)?;
        let mut passes: Vec<_> = records
            .pass_files()?
            .into_iter()
            .filter(|(instance, _)| records.pass(instance).ok().flatten().is_some())
            .map(|(instance, modified)| (modified, instance))
            .collect();
        passes.sort();
        let history = ExploreHistory {
            passes: passes.into_iter().map(|(_, instance)| instance).collect(),
            latest_editable: false,
        };
        records.save_history(&history)?;
        Ok(history)
    }

    /// Remove one unreadable pass and its editor view while preserving other passes.
    pub(crate) fn clear_pass(&self, unit: &ReviewUnit, instance: &str) -> Result<()> {
        let records = self.store.lock_explore(unit)?;
        let mut history = records.history()?;
        if history.passes.last().is_some_and(|saved| saved == instance) {
            history.latest_editable = false;
        }
        history.passes.retain(|saved| saved != instance);
        records.save_history(&history)?;
        records.remove_pass(instance)?;
        records.remove_view(instance)?;
        records.sync()
    }

    /// Discard a damaged editor view while retaining its valid interview.
    pub(crate) fn clear_view(&self, unit: &ReviewUnit, instance: &str) -> Result<()> {
        let records = self.store.lock_explore(unit)?;
        records.remove_view(instance)?;
        records.sync()
    }

    /// Save the reviewer's editor state without rewriting the pass. Older autosaves
    /// never replace newer ones, and one sequence number never changes content.
    pub(crate) fn save_view(&self, unit: &ReviewUnit, view: &ViewSave) -> Result<()> {
        if &view.review_unit != unit {
            return Err(explore_error("editor belongs to another review"));
        }
        let records = self.store.lock_explore(unit)?;
        if !records.history()?.passes.contains(&view.instance) {
            return Err(explore_error("saved pass is missing"));
        }
        if let Some(previous) = records.view(&view.instance)? {
            if previous.sequence == view.sequence && previous != *view {
                return Err(explore_error(
                    "editor sequence already has different content; original retained",
                ));
            }
            if previous.sequence >= view.sequence {
                return Ok(());
            }
        }
        records.save_view(view)
    }
}

#[cfg(test)]
#[path = "records.tests.rs"]
mod tests;
