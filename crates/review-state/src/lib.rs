//! Review status derived from repository snapshots and stored baselines.

mod review;

pub use review::{MarkResult, ReviewDiff, ReviewState, ReviewStatus, ReviewTracker, ReviewWarning};
