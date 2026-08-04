//! Git and jj repository snapshots, diffs, and excerpts.

pub mod diff;
pub mod excerpt;
pub mod repository;

mod error;

pub use error::{Error, Result};
