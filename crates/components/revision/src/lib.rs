//! Navigation between parent and child revisions.

mod core;
mod ui;

pub use core::RevisionComponent;

#[cfg(test)]
#[path = "lib.tests.rs"]
mod tests;
