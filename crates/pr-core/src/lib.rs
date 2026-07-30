//! Shared types and process boundaries for the progressive reviewer.

pub mod diff;
pub mod error;
pub mod excerpt;
pub mod herdr;
pub mod repository;
pub mod version;

pub use error::{Error, Result};
