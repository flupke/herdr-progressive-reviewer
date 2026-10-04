//! What each durable Explore change means: which round may change, when a conclusion
//! is final, how delivery outcomes are recorded and how damaged records are repaired.
//!
//! The store owns the record format and the per-review lock; every rule here runs
//! while that lock is held.

use review_explore::{
    DispatchResult, ExploreHistory, ExploreRound, InterviewUpdate, ReviewCompletion, ViewSave,
};
use review_explore_round_settings::WritingStyle;
use review_store::{Error, Result, ReviewStore};
use review_types::ReviewUnit;

fn explore_error(reason: &str) -> Error {
    Error::Explore(reason.into())
}

fn historical_round_error() -> Error {
    explore_error("this round is history; open the latest round to continue")
}

/// The rounds of a review started before a new one.
pub(crate) struct EarlierRounds {
    /// How many rounds the reviewer reset before a later round started.
    pub(crate) reset_rounds: usize,
    /// The rounds whose decisions still stand, oldest first.
    pub(crate) standing: Vec<ExploreRound>,
}

/// A round after an Explore response was submitted to it.
pub(crate) struct Submitted {
    /// False when the response was already accepted and changed nothing.
    pub(crate) applied: bool,
    pub(crate) round: ExploreRound,
}

/// The saved Explore rounds of every review, changed only through the session's rules.
#[derive(Clone)]
pub(crate) struct SavedRounds {
    store: ReviewStore,
}

impl SavedRounds {
    pub(crate) fn new(store: ReviewStore) -> Self {
        Self { store }
    }

    /// The writing style the reviewer's settings give the next round.
    pub(crate) fn next_writing(&self) -> Result<WritingStyle> {
        Ok(self.store.explore_round_settings()?.writing)
    }

    pub(crate) fn history(&self, unit: &ReviewUnit) -> Result<ExploreHistory> {
        self.store.load_explore_history(unit)
    }

    pub(crate) fn round(&self, unit: &ReviewUnit, instance: &str) -> Result<Option<ExploreRound>> {
        self.store.load_explore(unit, instance)
    }

    /// The rounds of `unit` started before `instance`: how many the reviewer reset before a
    /// later round started, and the readable rounds whose decisions still stand, oldest first.
    /// An unreadable round is left out.
    pub(crate) fn earlier(&self, unit: &ReviewUnit, instance: &str) -> Result<EarlierRounds> {
        let history = self.history(unit)?;
        let before = history.before(instance);
        Ok(EarlierRounds {
            reset_rounds: before.reset,
            standing: before
                .standing
                .iter()
                .filter_map(|saved| self.round(unit, saved).ok().flatten())
                .collect(),
        })
    }

    pub(crate) fn view(&self, unit: &ReviewUnit, instance: &str) -> Result<Option<ViewSave>> {
        self.store.load_explore_view(unit, instance)
    }

    /// Save a new round, then make it the editable latest round of its review.
    pub(crate) fn create(&self, round: ExploreRound) -> Result<ExploreRound> {
        let records = self
            .store
            .lock_explore(&round.exploration.comparison.checkpoint.review_unit)?;
        let mut history = records.history()?;
        records.create_round(&round)?;
        if history.closed {
            // The reviewer reset the latest round: this one starts over.
            history.reset_rounds = history.rounds.len();
        }
        history.rounds.push(round.exploration.instance.clone());
        history.latest_editable = true;
        history.closed = false;
        records.save_history(&history)?;
        Ok(round)
    }

    /// Change the editable latest round; earlier rounds are history.
    pub(crate) fn update<T>(
        &self,
        unit: &ReviewUnit,
        instance: &str,
        update: impl FnOnce(&mut ExploreRound) -> std::result::Result<T, String>,
    ) -> Result<(T, ExploreRound)> {
        let records = self.store.lock_explore(unit)?;
        if records.history()?.is_historical(instance) {
            return Err(historical_round_error());
        }
        records.update_round(instance, update)
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
            return Err(historical_round_error());
        }
        let (applied, round) = records.update_round(instance, |round| {
            let applied = round.submit(update).map_err(|error| error.to_string())?;
            if applied && update.conclusion.is_some() && round.completion.is_none() {
                round.completion = Some(ReviewCompletion {
                    request: update.request.clone(),
                    baseline: round.exploration.comparison.checkpoint.checkpoint.clone(),
                });
            }
            Ok(applied)
        })?;
        Ok(Submitted { applied, round })
    }

    /// A started external call may complete after a reset. Only its result can change history.
    pub(crate) fn finish_dispatch(
        &self,
        unit: &ReviewUnit,
        instance: &str,
        result: &DispatchResult,
    ) -> Result<ExploreRound> {
        let records = self.store.lock_explore(unit)?;
        if !records.history()?.rounds.iter().any(|id| id == instance) {
            return Err(explore_error("saved round is missing"));
        }
        records
            .update_round(instance, |round| {
                round.finish_dispatch(result);
                Ok(())
            })
            .map(|((), round)| round)
    }

    /// Rebuild a damaged index from readable rounds, oldest first, retaining their
    /// editor views. A rebuilt history never guesses which round is editable.
    pub(crate) fn repair_history(&self, unit: &ReviewUnit) -> Result<ExploreHistory> {
        let records = self.store.lock_explore(unit)?;
        let mut rounds: Vec<_> = records
            .round_files()?
            .into_iter()
            .filter(|(instance, _)| records.round(instance).ok().flatten().is_some())
            .map(|(instance, modified)| (modified, instance))
            .collect();
        rounds.sort();
        let history = ExploreHistory {
            rounds: rounds.into_iter().map(|(_, instance)| instance).collect(),
            latest_editable: false,
            ..ExploreHistory::default()
        };
        records.save_history(&history)?;
        Ok(history)
    }

    /// Close `instance` when it is the latest round, so reopening shows the start
    /// screen. Its records stay. A later round, another reviewer's, stays open.
    pub(crate) fn close(&self, unit: &ReviewUnit, instance: &str) -> Result<()> {
        let records = self.store.lock_explore(unit)?;
        let mut history = records.history()?;
        if history.latest() != Some(instance) || history.closed {
            return Ok(());
        }
        history.closed = true;
        records.save_history(&history)?;
        records.sync()
    }

    /// Remove one unreadable round and its editor view while preserving other rounds.
    pub(crate) fn clear_round(&self, unit: &ReviewUnit, instance: &str) -> Result<()> {
        let records = self.store.lock_explore(unit)?;
        let mut history = records.history()?;
        if history.rounds.last().is_some_and(|saved| saved == instance) {
            history.latest_editable = false;
        }
        history.rounds.retain(|saved| saved != instance);
        records.save_history(&history)?;
        records.remove_round(instance)?;
        records.remove_view(instance)?;
        records.sync()
    }

    /// Discard a damaged editor view while retaining its valid interview.
    pub(crate) fn clear_view(&self, unit: &ReviewUnit, instance: &str) -> Result<()> {
        let records = self.store.lock_explore(unit)?;
        records.remove_view(instance)?;
        records.sync()
    }

    /// Save the reviewer's editor state without rewriting the round. Older autosaves
    /// never replace newer ones, and one sequence number never changes content.
    pub(crate) fn save_view(&self, unit: &ReviewUnit, view: &ViewSave) -> Result<()> {
        if &view.review_unit != unit {
            return Err(explore_error("editor belongs to another review"));
        }
        let records = self.store.lock_explore(unit)?;
        if !records.history()?.rounds.contains(&view.instance) {
            return Err(explore_error("saved round is missing"));
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
