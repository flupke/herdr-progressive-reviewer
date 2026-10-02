//! Review status derived from repository snapshots and stored baselines.

mod open_rows;
mod progress;
mod review;

pub use open_rows::{OpenRow, RowChange};
pub use progress::ReviewProgress;
pub use review::{
    FileLines, MarkResult, OpenLines, ReviewDiff, ReviewState, ReviewStatus, ReviewTracker,
    ReviewWarning,
};
