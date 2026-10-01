//! Review status derived from repository snapshots and stored baselines.

mod progress;
mod review;

pub use progress::ReviewProgress;
pub use review::{
    FileLines, MarkResult, OpenLines, ReviewDiff, ReviewState, ReviewStatus, ReviewTracker,
    ReviewWarning,
};
