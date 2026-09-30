//! Whether changes judged insignificant may leave Explore's required coverage.

use std::sync::Arc;

use crate::{Comparison, JevMode, SignificanceResult};

/// The exclusion policy chosen once by the process that owns Explore.
///
/// Disabled keeps every changed line required. Enabled classifies changed windows;
/// a classification can only remove coverage obligations.
#[derive(Clone, Default)]
pub struct ExclusionPolicy(Option<Arc<dyn SignificanceClassifier>>);

impl std::fmt::Debug for ExclusionPolicy {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("ExclusionPolicy")
            .field(&self.mode())
            .finish()
    }
}

impl ExclusionPolicy {
    pub fn disabled() -> Self {
        Self(None)
    }

    pub fn enabled(classifier: Arc<dyn SignificanceClassifier>) -> Self {
        Self(Some(classifier))
    }

    pub fn is_enabled(&self) -> bool {
        self.0.is_some()
    }

    pub fn mode(&self) -> JevMode {
        if self.is_enabled() {
            JevMode::Enabled
        } else {
            JevMode::Disabled
        }
    }

    pub fn classifier(&self) -> Option<&dyn SignificanceClassifier> {
        self.0.as_deref()
    }
}

/// Judges whether each changed window of a comparison needs its own explanation.
pub trait SignificanceClassifier: Send + Sync {
    /// Names the decision rules; saved results from other rules are classified again.
    fn rubric(&self) -> &str;

    /// Divide `comparison` into windows, keeping for classification only those
    /// `classified` does not already hold.
    fn plan(&self, comparison: &Comparison, classified: &dyn Fn(&str) -> bool) -> SignificancePlan;
}

type Classify = Box<dyn FnOnce(&mut dyn FnMut(SignificanceResult) -> bool) -> bool + Send>;

/// Planned classification work, which can run on another thread.
pub struct SignificancePlan {
    total_windows: usize,
    classify: Classify,
}

impl SignificancePlan {
    /// `classify` records each result until the recorder returns false, and reports
    /// whether every planned window was recorded.
    pub fn new(
        total_windows: usize,
        classify: impl FnOnce(&mut dyn FnMut(SignificanceResult) -> bool) -> bool + Send + 'static,
    ) -> Self {
        Self {
            total_windows,
            classify: Box::new(classify),
        }
    }

    /// Every window of the comparison, including those already classified.
    pub fn total_windows(&self) -> usize {
        self.total_windows
    }

    /// True when every remaining window was classified and recorded.
    pub fn run(self, mut record: impl FnMut(SignificanceResult) -> bool) -> bool {
        (self.classify)(&mut record)
    }
}
