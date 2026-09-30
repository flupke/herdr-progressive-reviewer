//! Herdr protocol types, socket access, and agent targeting.

pub mod client;
#[cfg(any(test, feature = "memory"))]
pub mod memory;
pub mod protocol;

mod error;

pub use error::{Error, Result};
