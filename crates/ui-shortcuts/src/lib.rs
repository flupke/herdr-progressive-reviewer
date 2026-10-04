//! The shared shortcut table and its matcher over normalized keys.

mod commands;
mod matcher;
mod table;

pub use commands::{
    ApplicationCommand, ApplicationShortcut, CommentShortcut, ConversationCommand,
    ConversationShortcut, DiffGlobalShortcut, DiffPaneCommand, DiffShortcut, ExploreCommand,
    ExploreEvidenceShortcut, ExploreGlobalShortcut, ExploreSettingShortcut, ExploreShortcut,
    ExploreStartShortcut, ExploreTurnShortcut, FilesShortcut, HunkShortcut, LocationShortcut,
    LspShortcut, MovementShortcut, OverlayShortcut, RevisionShortcut, SearchMatchShortcut,
    SearchShortcut, ShortcutCommand, ShortcutSubscription, SourceShortcut, ThreadsCommand,
    ThreadsShortcut,
};
pub use matcher::ShortcutMatcher;
use table::{SHORTCUTS, ShortcutDefinition, ShortcutSequence, bindings};
pub use ui_keys::Key;

/// Return the visible shortcut help lines.
pub fn help_lines() -> impl Iterator<Item = (String, &'static str)> {
    SHORTCUTS.iter().filter_map(ShortcutDefinition::help_line)
}

pub fn closes_help(key: Key) -> bool {
    bindings().any(|binding| binding.closes_help && binding.sequence == ShortcutSequence::One(key))
}

pub fn help_close_label() -> String {
    bindings()
        .filter(|binding| binding.closes_help)
        .map(|binding| binding.sequence.label())
        .collect::<Vec<_>>()
        .join(" or ")
}

#[cfg(test)]
#[path = "lib.tests.rs"]
mod tests;
