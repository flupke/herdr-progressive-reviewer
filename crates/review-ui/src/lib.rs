//! Ratatui state, rendering, and input handling for progressive review.

use std::path::Path;

mod application;
mod application_frame;
mod layout;
mod message;

pub use application::ReviewApplication;
pub use application_frame::ApplicationFrame;
pub use message::UserInput;
pub use ui_actions::{Action, SourceLoadMode};
pub use ui_shortcuts::Key;
pub use ui_theme::Theme;

/// Return true when a path names a Rust source file.
pub fn is_rust_path(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("rs"))
}
