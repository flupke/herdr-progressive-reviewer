//! Ratatui state, rendering, and input handling for progressive review.

mod application;
mod application_frame;
mod layout;
mod message;
mod navigation;

pub use application::ReviewApplication;
pub use application_frame::ApplicationFrame;
pub use message::UserInput;
pub use ui_actions::{
    Action, DocumentAction, DocumentLoad, LspAction, RepositoryAction, SettingsAction,
    SourceLoadMode, TerminalAction,
};
pub use ui_shortcuts::Key;
pub use ui_theme::Theme;
