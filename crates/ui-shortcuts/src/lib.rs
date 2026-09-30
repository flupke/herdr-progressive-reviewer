//! Normalized keyboard input and the shared shortcut table.

mod commands;
mod matcher;
mod table;

pub use commands::{
    ApplicationCommand, ApplicationShortcut, CommentShortcut, ConversationCommand,
    ConversationShortcut, DiffGlobalShortcut, DiffPaneCommand, DiffShortcut, ExploreCommand,
    ExploreCoverageShortcut, ExploreEvidenceShortcut, ExploreGlobalShortcut, ExploreShortcut,
    ExploreTurnShortcut, FilesShortcut, HunkShortcut, LocationShortcut, LspShortcut,
    MovementShortcut, OverlayShortcut, RevisionShortcut, SearchMatchShortcut, SearchShortcut,
    ShortcutCommand, ShortcutSubscription, SourceShortcut, ThreadsCommand, ThreadsShortcut,
};
pub use matcher::ShortcutMatcher;
use table::{SHORTCUTS, ShortcutDefinition, ShortcutSequence, bindings};

/// One normalized keyboard input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Key {
    Char(char),
    Control(char),
    Alt(char),
    Backspace,
    Delete,
    Tab,
    Left,
    Right,
    Down,
    Up,
    PageDown,
    PageUp,
    First,
    Last,
    HalfPageDown,
    HalfPageUp,
    PreviousLocation,
    NextLocation,
    Visual,
    Expand,
    CommitMessage,
    Escape,
    Enter,
    ControlEnter,
    EditorMode,
    Space,
    Quit,
}

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

impl Key {
    fn label(self) -> String {
        match self {
            Self::Char(character) => return character.to_string(),
            Self::Control(character) => return format!("Ctrl-{character}"),
            Self::Alt(character) => return format!("Alt-{character}"),
            _ => {}
        }
        NAMED_KEY_LABELS
            .iter()
            .find_map(|(candidate, label)| (*candidate == self).then_some(*label))
            .expect("each non-character key must have a label")
            .to_owned()
    }
}

const NAMED_KEY_LABELS: &[(Key, &str)] = &[
    (Key::Left, "Left"),
    (Key::Right, "Right"),
    (Key::Delete, "Delete"),
    (Key::PageDown, "PageDown"),
    (Key::PageUp, "PageUp"),
    (Key::ControlEnter, "Ctrl-Enter"),
    (Key::EditorMode, "F2"),
    (Key::Backspace, "Backspace"),
    (Key::Tab, "Tab"),
    (Key::Down, "Down"),
    (Key::Up, "Up"),
    (Key::First, "Home"),
    (Key::Last, "End"),
    (Key::HalfPageDown, "Ctrl-d"),
    (Key::HalfPageUp, "Ctrl-u"),
    (Key::PreviousLocation, "Ctrl-o"),
    (Key::NextLocation, "Ctrl-i"),
    (Key::Visual, "V"),
    (Key::Expand, "l"),
    (Key::CommitMessage, "c"),
    (Key::Escape, "Esc"),
    (Key::Enter, "Enter"),
    (Key::Space, "Space"),
    (Key::Quit, "q"),
];

#[cfg(test)]
#[path = "lib.tests.rs"]
mod tests;
