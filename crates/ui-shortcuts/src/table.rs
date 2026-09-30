//! The shortcut table: each command's bindings, help text and owning scope.

use crate::Key;
use crate::commands::{
    ApplicationShortcut, CommentShortcut, DiffGlobalShortcut, DiffShortcut, FilesShortcut,
    HunkShortcut, LocationShortcut, LspShortcut, MovementShortcut, OverlayShortcut,
    RevisionShortcut, SearchMatchShortcut, SearchShortcut, ShortcutCommand, SourceShortcut,
};

const fn application(command: ApplicationShortcut) -> ShortcutCommand {
    ShortcutCommand::Application(command)
}

const fn search(command: SearchShortcut) -> ShortcutCommand {
    ShortcutCommand::Search(command)
}

const fn movement(command: MovementShortcut) -> ShortcutCommand {
    ShortcutCommand::Movement(command)
}

const fn diff(command: DiffShortcut) -> ShortcutCommand {
    ShortcutCommand::Diff(command)
}

const fn comment(command: CommentShortcut) -> ShortcutCommand {
    diff(DiffShortcut::Comment(command))
}

const fn location(command: LocationShortcut) -> ShortcutCommand {
    diff(DiffShortcut::Location(command))
}

const fn search_match(command: SearchMatchShortcut) -> ShortcutCommand {
    diff(DiffShortcut::SearchMatch(command))
}

const fn source(command: SourceShortcut) -> ShortcutCommand {
    diff(DiffShortcut::Source(command))
}

const fn lsp(command: LspShortcut) -> ShortcutCommand {
    diff(DiffShortcut::Lsp(command))
}

const fn diff_global(command: DiffGlobalShortcut) -> ShortcutCommand {
    ShortcutCommand::DiffGlobal(command)
}

const fn hunk(command: HunkShortcut) -> ShortcutCommand {
    diff_global(DiffGlobalShortcut::Hunk(command))
}

const fn files(command: FilesShortcut) -> ShortcutCommand {
    ShortcutCommand::Files(command)
}

const fn overlay(command: OverlayShortcut) -> ShortcutCommand {
    ShortcutCommand::Overlay(command)
}

const fn revision(command: RevisionShortcut) -> ShortcutCommand {
    ShortcutCommand::Revision(command)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ShortcutSequence {
    One(Key),
    Two(Key, Key),
}

impl ShortcutSequence {
    const fn one(key: Key) -> Self {
        Self::One(key)
    }

    const fn two(first: Key, second: Key) -> Self {
        Self::Two(first, second)
    }

    pub(crate) fn starts_with(self, key: Key) -> bool {
        matches!(self, Self::Two(first, _) if first == key)
    }

    pub(crate) fn matches(self, prefix: Option<Key>, key: Key) -> bool {
        match (self, prefix) {
            (Self::One(expected), None) => expected == key,
            (Self::Two(first, second), Some(prefix)) => first == prefix && second == key,
            _ => false,
        }
    }

    pub(crate) fn label(self) -> String {
        match self {
            Self::One(key) => key.label(),
            Self::Two(first, second) => format!("{}{}", first.label(), second.label()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ShortcutBinding {
    pub(crate) sequence: ShortcutSequence,
    pub(crate) command: ShortcutCommand,
    pub(crate) show_in_help: bool,
    pub(crate) closes_help: bool,
}

impl ShortcutBinding {
    const fn one(key: Key, command: ShortcutCommand) -> Self {
        Self {
            sequence: ShortcutSequence::one(key),
            command,
            show_in_help: true,
            closes_help: false,
        }
    }

    const fn two(first: Key, second: Key, command: ShortcutCommand) -> Self {
        Self {
            sequence: ShortcutSequence::two(first, second),
            command,
            show_in_help: true,
            closes_help: false,
        }
    }

    const fn alias(key: Key, command: ShortcutCommand) -> Self {
        Self {
            sequence: ShortcutSequence::one(key),
            command,
            show_in_help: false,
            closes_help: false,
        }
    }

    const fn help_close(key: Key, command: ShortcutCommand) -> Self {
        Self {
            sequence: ShortcutSequence::one(key),
            command,
            show_in_help: !matches!(key, Key::Escape),
            closes_help: true,
        }
    }
}

pub(crate) struct ShortcutDefinition {
    pub(crate) description: Option<&'static str>,
    pub(crate) bindings: &'static [ShortcutBinding],
}

impl ShortcutDefinition {
    pub(crate) fn help_line(&self) -> Option<(String, &'static str)> {
        let description = self.description?;
        let labels = self
            .bindings
            .iter()
            .filter(|binding| binding.show_in_help)
            .map(|binding| binding.sequence.label())
            .collect::<Vec<_>>();
        let separator = if labels.first().is_some_and(|label| label == "/") {
            ", "
        } else {
            " / "
        };
        let keys = labels.join(separator);
        Some((keys, description))
    }
}

/// Every binding in table order.
pub(crate) fn bindings() -> impl Iterator<Item = &'static ShortcutBinding> {
    SHORTCUTS.iter().flat_map(|definition| definition.bindings)
}

pub(crate) const SHORTCUTS: &[ShortcutDefinition] = &[
    ShortcutDefinition {
        description: Some("Open Files / Threads / Explore"),
        bindings: &[
            ShortcutBinding::one(Key::Char('f'), application(ApplicationShortcut::OpenFiles)),
            ShortcutBinding::alias(Key::Char('F'), application(ApplicationShortcut::OpenFiles)),
            ShortcutBinding::one(
                Key::Char('t'),
                application(ApplicationShortcut::OpenThreads),
            ),
            ShortcutBinding::alias(
                Key::Char('T'),
                application(ApplicationShortcut::OpenThreads),
            ),
            ShortcutBinding::one(
                Key::Char('e'),
                application(ApplicationShortcut::OpenExplore),
            ),
        ],
    },
    ShortcutDefinition {
        description: Some("Switch Files / Threads / Explore, including while composing"),
        bindings: &[ShortcutBinding::one(
            Key::Control('t'),
            application(ApplicationShortcut::ToggleNavigation),
        )],
    },
    ShortcutDefinition {
        description: Some("Jump to first unread thread in All"),
        bindings: &[ShortcutBinding::one(
            Key::Alt('u'),
            application(ApplicationShortcut::NewReplies),
        )],
    },
    ShortcutDefinition {
        description: Some("Add / reply to comment"),
        bindings: &[
            ShortcutBinding::one(Key::Char('a'), comment(CommentShortcut::Add)),
            ShortcutBinding::one(Key::Char('A'), comment(CommentShortcut::Reply)),
        ],
    },
    ShortcutDefinition {
        description: Some("Previous / next comment"),
        bindings: &[
            ShortcutBinding::two(
                Key::Char('['),
                Key::Char('c'),
                diff_global(DiffGlobalShortcut::PreviousComment),
            ),
            ShortcutBinding::two(
                Key::Char(']'),
                Key::Char('c'),
                diff_global(DiffGlobalShortcut::NextComment),
            ),
        ],
    },
    ShortcutDefinition {
        description: Some("Move"),
        bindings: &[
            ShortcutBinding::one(Key::Char('j'), movement(MovementShortcut::MoveDown)),
            ShortcutBinding::one(Key::Char('k'), movement(MovementShortcut::MoveUp)),
            ShortcutBinding::one(Key::Down, movement(MovementShortcut::MoveDown)),
            ShortcutBinding::one(Key::Up, movement(MovementShortcut::MoveUp)),
        ],
    },
    ShortcutDefinition {
        description: Some("Go to first / last item"),
        bindings: &[
            ShortcutBinding::two(
                Key::Char('g'),
                Key::Char('g'),
                movement(MovementShortcut::GoToFirst),
            ),
            ShortcutBinding::alias(Key::First, movement(MovementShortcut::GoToFirst)),
            ShortcutBinding::one(Key::Char('G'), movement(MovementShortcut::GoToLast)),
            ShortcutBinding::alias(Key::Last, movement(MovementShortcut::GoToLast)),
        ],
    },
    ShortcutDefinition {
        description: Some("Move half a page"),
        bindings: &[
            ShortcutBinding::one(
                Key::HalfPageDown,
                movement(MovementShortcut::MoveHalfPageDown),
            ),
            ShortcutBinding::alias(Key::PageDown, movement(MovementShortcut::MoveHalfPageDown)),
            ShortcutBinding::one(Key::HalfPageUp, movement(MovementShortcut::MoveHalfPageUp)),
            ShortcutBinding::alias(Key::PageUp, movement(MovementShortcut::MoveHalfPageUp)),
        ],
    },
    ShortcutDefinition {
        description: Some("Go to previous / next location"),
        bindings: &[
            ShortcutBinding::one(
                Key::PreviousLocation,
                location(LocationShortcut::GoToPrevious),
            ),
            ShortcutBinding::one(Key::NextLocation, location(LocationShortcut::GoToNext)),
        ],
    },
    ShortcutDefinition {
        description: Some("Select revisions / go to a parent / child revision"),
        bindings: &[
            ShortcutBinding::two(
                Key::Char('v'),
                Key::Char('v'),
                revision(RevisionShortcut::OpenSelector),
            ),
            ShortcutBinding::two(
                Key::Char('['),
                Key::Char('v'),
                revision(RevisionShortcut::GoToParent),
            ),
            ShortcutBinding::two(
                Key::Char(']'),
                Key::Char('v'),
                revision(RevisionShortcut::GoToChild),
            ),
        ],
    },
    ShortcutDefinition {
        description: Some("Change focused pane"),
        bindings: &[ShortcutBinding::one(
            Key::Tab,
            application(ApplicationShortcut::ChangeFocus),
        )],
    },
    ShortcutDefinition {
        description: Some("Move within source text"),
        bindings: &[
            ShortcutBinding::one(Key::Char('h'), source(SourceShortcut::MoveLeft)),
            ShortcutBinding::one(Key::Char('l'), source(SourceShortcut::ExpandOrMoveRight)),
            ShortcutBinding::one(Key::Char('w'), source(SourceShortcut::MoveToNextWord)),
            ShortcutBinding::one(Key::Char('b'), source(SourceShortcut::MoveToPreviousWord)),
            ShortcutBinding::one(Key::Char('0'), source(SourceShortcut::MoveToStartOfLine)),
            ShortcutBinding::one(Key::Char('$'), source(SourceShortcut::MoveToEndOfLine)),
        ],
    },
    ShortcutDefinition {
        description: Some("Expand an unchanged section"),
        bindings: &[ShortcutBinding::one(
            Key::Expand,
            source(SourceShortcut::ExpandOrMoveRight),
        )],
    },
    ShortcutDefinition {
        description: Some("Select diff lines"),
        bindings: &[
            ShortcutBinding::alias(Key::Visual, diff(DiffShortcut::StartSelection)),
            ShortcutBinding::one(Key::Char('V'), diff(DiffShortcut::StartSelection)),
        ],
    },
    ShortcutDefinition {
        description: Some("Mark a file as reviewed"),
        bindings: &[
            ShortcutBinding::one(Key::Space, files(FilesShortcut::MarkReviewed)),
            ShortcutBinding::alias(Key::Char(' '), files(FilesShortcut::MarkReviewed)),
        ],
    },
    ShortcutDefinition {
        description: Some("Jev: mark insignificant files reviewed"),
        bindings: &[ShortcutBinding::two(
            Key::Char('r'),
            Key::Char('f'),
            files(FilesShortcut::AutoReview),
        )],
    },
    ShortcutDefinition {
        description: Some("Set all files to unreviewed (confirm)"),
        bindings: &[ShortcutBinding::two(
            Key::Char('r'),
            Key::Char('U'),
            files(FilesShortcut::UnreviewAll),
        )],
    },
    ShortcutDefinition {
        description: Some("Search / word, next match, previous match"),
        bindings: &[
            ShortcutBinding::one(Key::Char('/'), search(SearchShortcut::Begin)),
            ShortcutBinding::one(
                Key::Char('*'),
                search_match(SearchMatchShortcut::WordUnderCursor),
            ),
            ShortcutBinding::one(Key::Char('n'), search_match(SearchMatchShortcut::NextMatch)),
            ShortcutBinding::one(
                Key::Char('p'),
                search_match(SearchMatchShortcut::PreviousMatch),
            ),
        ],
    },
    ShortcutDefinition {
        description: Some("Show symbol documentation"),
        bindings: &[ShortcutBinding::one(
            Key::Char('K'),
            lsp(LspShortcut::ShowDocumentation),
        )],
    },
    ShortcutDefinition {
        description: Some("Definition / type definition / references / restart LSP"),
        bindings: &[
            ShortcutBinding::two(
                Key::Char('g'),
                Key::Char('d'),
                lsp(LspShortcut::GoToDefinition),
            ),
            ShortcutBinding::two(
                Key::Char('g'),
                Key::Char('y'),
                lsp(LspShortcut::GoToTypeDefinition),
            ),
            ShortcutBinding::two(
                Key::Char('g'),
                Key::Char('r'),
                lsp(LspShortcut::GoToReferences),
            ),
            ShortcutBinding::two(Key::Char('g'), Key::Char('R'), lsp(LspShortcut::Restart)),
        ],
    },
    ShortcutDefinition {
        description: Some("Go to previous / next modified hunk"),
        bindings: &[
            ShortcutBinding::two(
                Key::Char('['),
                Key::Char('h'),
                hunk(HunkShortcut::GoToPreviousModified),
            ),
            ShortcutBinding::two(
                Key::Char(']'),
                Key::Char('h'),
                hunk(HunkShortcut::GoToNextModified),
            ),
        ],
    },
    ShortcutDefinition {
        description: Some("Go to previous / next unreviewed file"),
        bindings: &[
            ShortcutBinding::two(
                Key::Char('['),
                Key::Char('f'),
                files(FilesShortcut::GoToPreviousUnreviewed),
            ),
            ShortcutBinding::two(
                Key::Char(']'),
                Key::Char('f'),
                files(FilesShortcut::GoToNextUnreviewed),
            ),
        ],
    },
    ShortcutDefinition {
        description: Some("Open the current file in $EDITOR"),
        bindings: &[ShortcutBinding::one(
            Key::Char('E'),
            diff_global(DiffGlobalShortcut::OpenInEditor),
        )],
    },
    ShortcutDefinition {
        description: Some("Show the commit message"),
        bindings: &[
            ShortcutBinding::alias(
                Key::CommitMessage,
                overlay(OverlayShortcut::ShowCommitMessage),
            ),
            ShortcutBinding::one(Key::Char('c'), overlay(OverlayShortcut::ShowCommitMessage)),
        ],
    },
    ShortcutDefinition {
        description: Some("Quit"),
        bindings: &[
            ShortcutBinding::alias(Key::Quit, application(ApplicationShortcut::Quit)),
            ShortcutBinding::one(Key::Char('q'), application(ApplicationShortcut::Quit)),
        ],
    },
    ShortcutDefinition {
        description: Some("Show keyboard shortcuts"),
        bindings: &[ShortcutBinding::help_close(
            Key::Char('?'),
            overlay(OverlayShortcut::OpenHelp),
        )],
    },
    ShortcutDefinition {
        description: None,
        bindings: &[ShortcutBinding::help_close(
            Key::Escape,
            application(ApplicationShortcut::Clear),
        )],
    },
];
