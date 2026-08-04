//! Ratatui state, rendering, and input handling for progressive review.

use std::path::Path;

mod app;
mod commit_message;
mod context_menu;
mod diff;
mod file_tree;
mod files;
mod footer;
mod header;
mod highlight;
mod hover;
mod input;
mod navigation;
mod presentation;
mod render;
mod review_view;
#[cfg(test)]
mod tests;
mod theme;

pub use app::{Action, Key, Message, ReviewApp, ReviewFile, SourceLoadMode};
pub use review_view::ReviewView;
pub use theme::Theme;

/// Return true when a path names a Rust source file.
pub fn is_rust_path(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("rs"))
}
