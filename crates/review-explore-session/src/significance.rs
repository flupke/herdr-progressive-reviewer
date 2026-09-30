//! Background significance classification, when the exclusion policy enables it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use review_explore::{Comparison, ExplorePass};

use crate::{ExploreSession, publish_committed};

impl ExploreSession {
    /// Start classifying the pass's changes unless it is finished, already running or
    /// already classified under the current rubric.
    pub(crate) fn start_classification_if_enabled(&mut self, pass: ExplorePass) -> ExplorePass {
        let Some(classifier) = self.exclusion.classifier() else {
            return pass;
        };
        let rubric = classifier.rubric().to_owned();
        if pass.completion.is_some()
            || self.classification_running(&pass.exploration.instance)
            || !pass.coverage.needs_classification(&rubric, pass.revision)
        {
            return pass;
        }
        let Some(comparison) = self.classification_comparison(&pass) else {
            return pass;
        };
        let plan = classifier.plan(&comparison, &|window| pass.coverage.is_classified(window));
        let total_windows = plan.total_windows();
        let unit = pass.exploration.comparison.checkpoint.review_unit.clone();
        let instance = pass.exploration.instance.clone();
        let attempt = uuid::Uuid::new_v4().to_string();
        let Ok((started, pass)) = self.store.update_explore(&unit, &instance, |pass| {
            if pass.completion.is_some()
                || !pass.coverage.needs_classification(&rubric, pass.revision)
            {
                return Ok(false);
            }
            pass.coverage
                .start_classification(&rubric, attempt.clone(), total_windows);
            Ok(true)
        }) else {
            return pass;
        };
        if !started {
            return pass;
        }
        let prior_elapsed = pass
            .coverage
            .classification_progress()
            .map_or(0, |progress| progress.elapsed_ms);
        let active = Arc::new(AtomicBool::new(true));
        self.state.classification = Some((instance.clone(), active.clone()));
        let store = self.store.clone();
        let events = self.events.clone();
        std::thread::spawn(move || {
            let started = std::time::Instant::now();
            let elapsed = || {
                prior_elapsed.saturating_add(
                    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                )
            };
            let finished = plan.run(|result| {
                match store.update_explore(&unit, &instance, |pass| {
                    if !pass.coverage.is_current_attempt(&attempt) || pass.completion.is_some() {
                        return Err("obsolete classification attempt".into());
                    }
                    Ok(pass
                        .coverage
                        .record_classification_progress(result, elapsed()))
                }) {
                    Ok((true, pass)) => publish_committed(&events, pass),
                    Ok((false, _)) => true,
                    Err(_) => false,
                }
            });
            if let Ok((_, pass)) = store.update_explore(&unit, &instance, |pass| {
                if !pass.coverage.is_current_attempt(&attempt) {
                    return Ok(false);
                }
                let now = std::time::SystemTime::now();
                if finished {
                    pass.coverage.finish_classification(elapsed(), now);
                } else {
                    pass.coverage.stop_classification(elapsed(), now);
                }
                Ok(true)
            }) {
                publish_committed(&events, pass);
            }
            active.store(false, Ordering::Relaxed);
        });
        pass
    }

    fn classification_running(&self, instance: &str) -> bool {
        self.state
            .classification
            .as_ref()
            .is_some_and(|(current, active)| current == instance && active.load(Ordering::Relaxed))
    }

    /// Saved comparisons omit diff bytes; recapture only the same checkpoint.
    fn classification_comparison(&self, pass: &ExplorePass) -> Option<Arc<Comparison>> {
        let comparison = match &self.state.comparison {
            Some(comparison) if comparison.diffs.len() == comparison.files.len() => {
                comparison.clone()
            }
            _ => self.capture().ok()?,
        };
        (comparison.checkpoint == pass.exploration.comparison.checkpoint).then_some(comparison)
    }
}
