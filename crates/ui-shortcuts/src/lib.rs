//! Normalized keyboard input and the shared shortcut table.

use component_core::{InputMatcher, InputResolution};

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShortcutCommand {
    Application(ApplicationShortcut),
    Comment(CommentShortcut),
    File(FileShortcut),
    Guide(GuideShortcut),
    Hunk(HunkShortcut),
    Lsp(LspShortcut),
    Navigation(NavigationShortcut),
    Search(SearchShortcut),
    Source(SourceShortcut),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommentShortcut {
    Add,
    Reply,
    Previous,
    Next,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HunkShortcut {
    GoToNextModified,
    GoToPreviousModified,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileShortcut {
    GoToNextUnreviewed,
    GoToPreviousUnreviewed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplicationShortcut {
    ChangeFocus,
    OpenFiles,
    OpenThreads,
    OpenExplore,
    ToggleNavigation,
    NewReplies,
    Clear,
    MarkReviewed,
    OpenHelp,
    Quit,
    ShowCommitMessage,
    StartSelection,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GuideShortcut {
    GenerateFull,
    GenerateSelectedFile,
    GoToNextComment,
    GoToPreviousComment,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LspShortcut {
    GoToDefinition,
    GoToTypeDefinition,
    GoToReferences,
    Restart,
    ShowDocumentation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NavigationShortcut {
    GoToFirst,
    GoToLast,
    GoToNextLocation,
    GoToChildRevision,
    GoToParentRevision,
    GoToPreviousLocation,
    MoveDown,
    MoveHalfPageDown,
    MoveHalfPageUp,
    MoveUp,
    OpenRevisionSelector,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SearchShortcut {
    Begin,
    NextMatch,
    PreviousMatch,
    WordUnderCursor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceShortcut {
    ExpandOrMoveRight,
    MoveLeft,
    MoveToEndOfLine,
    MoveToNextWord,
    MoveToPreviousWord,
    MoveToStartOfLine,
}

const fn application(command: ApplicationShortcut) -> ShortcutCommand {
    ShortcutCommand::Application(command)
}

const fn guide(command: GuideShortcut) -> ShortcutCommand {
    ShortcutCommand::Guide(command)
}

const fn lsp(command: LspShortcut) -> ShortcutCommand {
    ShortcutCommand::Lsp(command)
}

const fn navigation(command: NavigationShortcut) -> ShortcutCommand {
    ShortcutCommand::Navigation(command)
}

const fn search(command: SearchShortcut) -> ShortcutCommand {
    ShortcutCommand::Search(command)
}

const fn source(command: SourceShortcut) -> ShortcutCommand {
    ShortcutCommand::Source(command)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ShortcutSequence {
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

    fn starts_with(self, key: Key) -> bool {
        matches!(self, Self::Two(first, _) if first == key)
    }

    fn matches(self, prefix: Option<ShortcutPrefix>, key: Key) -> bool {
        match (self, prefix) {
            (Self::One(expected), None) => expected == key,
            (Self::Two(first, second), Some(prefix)) => first == prefix.key() && second == key,
            _ => false,
        }
    }

    fn label(self) -> String {
        match self {
            Self::One(key) => key_label(key),
            Self::Two(first, second) => format!("{}{}", key_label(first), key_label(second)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShortcutPrefix(Key);

impl ShortcutPrefix {
    const fn new(key: Key) -> Self {
        Self(key)
    }

    const fn key(self) -> Key {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ShortcutBinding {
    sequence: ShortcutSequence,
    command: ShortcutCommand,
    show_in_help: bool,
    closes_help: bool,
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

struct ShortcutDefinition {
    description: Option<&'static str>,
    bindings: &'static [ShortcutBinding],
}

impl ShortcutDefinition {
    fn help_line(&self) -> Option<(String, &'static str)> {
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShortcutLookup {
    Command(ShortcutCommand),
    Prefix(ShortcutPrefix),
    None,
}

/// The shortcut group owned by one input subscription.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShortcutSet {
    Comments,
    Application,
    Diff,
    Files,
    FilesGlobal,
    Guide,
    Hunk,
    Overlay,
    Revision,
}

/// Stateful matching for one shortcut subscription.
pub struct ShortcutMatcher {
    set: ShortcutSet,
    prefix: Option<ShortcutPrefix>,
}

impl ShortcutMatcher {
    pub const fn new(set: ShortcutSet) -> Self {
        Self { set, prefix: None }
    }

    pub fn resolve_key(&mut self, key: Key) -> InputResolution<ShortcutCommand> {
        let prefix = self.prefix.take();
        if let Some(command) = self.matching_command(prefix, key) {
            return InputResolution::Matched(command);
        }
        if prefix.is_some()
            && let Some(command) = self.matching_command(None, key)
        {
            return InputResolution::Matched(command);
        }
        if self.starts_sequence(key) {
            self.prefix = Some(ShortcutPrefix::new(key));
            InputResolution::AwaitingMoreInput
        } else {
            InputResolution::NoMatch
        }
    }

    fn matching_command(
        &self,
        prefix: Option<ShortcutPrefix>,
        key: Key,
    ) -> Option<ShortcutCommand> {
        for binding in SHORTCUTS.iter().flat_map(|definition| definition.bindings) {
            if !self.set.includes(binding.command) {
                continue;
            }
            if binding.sequence.matches(prefix, key) {
                return Some(binding.command);
            }
        }
        None
    }

    fn starts_sequence(&self, key: Key) -> bool {
        SHORTCUTS
            .iter()
            .flat_map(|definition| definition.bindings)
            .any(|binding| self.set.includes(binding.command) && binding.sequence.starts_with(key))
    }
}

impl<C> InputMatcher<C, Key> for ShortcutMatcher {
    type Output = ShortcutCommand;

    fn resolve(&mut self, _component: &C, key: &Key) -> InputResolution<Self::Output> {
        self.resolve_key(*key)
    }
}

impl ShortcutSet {
    const fn includes(self, command: ShortcutCommand) -> bool {
        if let Self::Comments = self {
            return matches!(
                command,
                ShortcutCommand::Comment(CommentShortcut::Previous | CommentShortcut::Next)
            );
        }
        if let Self::Application = self {
            return matches!(
                command,
                ShortcutCommand::Application(
                    ApplicationShortcut::ChangeFocus
                        | ApplicationShortcut::OpenFiles
                        | ApplicationShortcut::OpenThreads
                        | ApplicationShortcut::OpenExplore
                        | ApplicationShortcut::ToggleNavigation
                        | ApplicationShortcut::NewReplies
                        | ApplicationShortcut::Clear
                        | ApplicationShortcut::Quit
                ) | ShortcutCommand::Search(SearchShortcut::Begin)
            );
        }
        self.includes_other(command)
    }

    const fn includes_other(self, command: ShortcutCommand) -> bool {
        match self {
            Self::Diff => {
                !is_component_global_shortcut(command)
                    && !matches!(
                        command,
                        ShortcutCommand::Application(
                            ApplicationShortcut::ChangeFocus
                                | ApplicationShortcut::OpenFiles
                                | ApplicationShortcut::OpenThreads
                                | ApplicationShortcut::OpenExplore
                                | ApplicationShortcut::ToggleNavigation
                                | ApplicationShortcut::NewReplies
                                | ApplicationShortcut::Clear
                                | ApplicationShortcut::Quit
                        )
                    )
            }
            Self::Files => is_files_shortcut(command),
            Self::FilesGlobal => matches!(
                command,
                ShortcutCommand::File(_)
                    | ShortcutCommand::Application(ApplicationShortcut::MarkReviewed)
            ),
            Self::Guide => matches!(command, ShortcutCommand::Guide(_)),
            Self::Hunk => matches!(command, ShortcutCommand::Hunk(_)),
            Self::Overlay => matches!(
                command,
                ShortcutCommand::Application(
                    ApplicationShortcut::ShowCommitMessage | ApplicationShortcut::OpenHelp
                )
            ),
            Self::Revision => is_revision_shortcut(command),
            Self::Comments | Self::Application => false,
        }
    }
}

const SHORTCUTS: &[ShortcutDefinition] = &[
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
            ShortcutBinding::alias(
                Key::Char('E'),
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
            ShortcutBinding::one(
                Key::Char('a'),
                ShortcutCommand::Comment(CommentShortcut::Add),
            ),
            ShortcutBinding::one(
                Key::Char('A'),
                ShortcutCommand::Comment(CommentShortcut::Reply),
            ),
        ],
    },
    ShortcutDefinition {
        description: Some("Previous / next comment"),
        bindings: &[
            ShortcutBinding::two(
                Key::Char('['),
                Key::Char('c'),
                ShortcutCommand::Comment(CommentShortcut::Previous),
            ),
            ShortcutBinding::two(
                Key::Char(']'),
                Key::Char('c'),
                ShortcutCommand::Comment(CommentShortcut::Next),
            ),
        ],
    },
    ShortcutDefinition {
        description: Some("Move"),
        bindings: &[
            ShortcutBinding::one(Key::Char('j'), navigation(NavigationShortcut::MoveDown)),
            ShortcutBinding::one(Key::Char('k'), navigation(NavigationShortcut::MoveUp)),
            ShortcutBinding::one(Key::Down, navigation(NavigationShortcut::MoveDown)),
            ShortcutBinding::one(Key::Up, navigation(NavigationShortcut::MoveUp)),
        ],
    },
    ShortcutDefinition {
        description: Some("Go to first / last item"),
        bindings: &[
            ShortcutBinding::two(
                Key::Char('g'),
                Key::Char('g'),
                navigation(NavigationShortcut::GoToFirst),
            ),
            ShortcutBinding::alias(Key::First, navigation(NavigationShortcut::GoToFirst)),
            ShortcutBinding::one(Key::Char('G'), navigation(NavigationShortcut::GoToLast)),
            ShortcutBinding::alias(Key::Last, navigation(NavigationShortcut::GoToLast)),
        ],
    },
    ShortcutDefinition {
        description: Some("Move half a page"),
        bindings: &[
            ShortcutBinding::one(
                Key::HalfPageDown,
                navigation(NavigationShortcut::MoveHalfPageDown),
            ),
            ShortcutBinding::alias(
                Key::PageDown,
                navigation(NavigationShortcut::MoveHalfPageDown),
            ),
            ShortcutBinding::one(
                Key::HalfPageUp,
                navigation(NavigationShortcut::MoveHalfPageUp),
            ),
            ShortcutBinding::alias(Key::PageUp, navigation(NavigationShortcut::MoveHalfPageUp)),
        ],
    },
    ShortcutDefinition {
        description: Some("Go to previous / next location"),
        bindings: &[
            ShortcutBinding::one(
                Key::PreviousLocation,
                navigation(NavigationShortcut::GoToPreviousLocation),
            ),
            ShortcutBinding::one(
                Key::NextLocation,
                navigation(NavigationShortcut::GoToNextLocation),
            ),
        ],
    },
    ShortcutDefinition {
        description: Some("Select revisions / go to a parent / child revision"),
        bindings: &[
            ShortcutBinding::two(
                Key::Char('v'),
                Key::Char('v'),
                navigation(NavigationShortcut::OpenRevisionSelector),
            ),
            ShortcutBinding::two(
                Key::Char('['),
                Key::Char('v'),
                navigation(NavigationShortcut::GoToParentRevision),
            ),
            ShortcutBinding::two(
                Key::Char(']'),
                Key::Char('v'),
                navigation(NavigationShortcut::GoToChildRevision),
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
            ShortcutBinding::alias(
                Key::Visual,
                application(ApplicationShortcut::StartSelection),
            ),
            ShortcutBinding::alias(
                Key::Char('V'),
                application(ApplicationShortcut::StartSelection),
            ),
        ],
    },
    ShortcutDefinition {
        description: Some("Mark a file as reviewed"),
        bindings: &[
            ShortcutBinding::one(Key::Space, application(ApplicationShortcut::MarkReviewed)),
            ShortcutBinding::alias(
                Key::Char(' '),
                application(ApplicationShortcut::MarkReviewed),
            ),
        ],
    },
    ShortcutDefinition {
        description: Some("Search / word, next match, previous match"),
        bindings: &[
            ShortcutBinding::one(Key::Char('/'), search(SearchShortcut::Begin)),
            ShortcutBinding::one(Key::Char('*'), search(SearchShortcut::WordUnderCursor)),
            ShortcutBinding::one(Key::Char('n'), search(SearchShortcut::NextMatch)),
            ShortcutBinding::one(Key::Char('p'), search(SearchShortcut::PreviousMatch)),
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
        description: Some("Generate a file / full review guide"),
        bindings: &[
            ShortcutBinding::two(
                Key::Char('r'),
                Key::Char('f'),
                guide(GuideShortcut::GenerateSelectedFile),
            ),
            ShortcutBinding::two(
                Key::Char('r'),
                Key::Char('a'),
                guide(GuideShortcut::GenerateFull),
            ),
        ],
    },
    ShortcutDefinition {
        description: Some("Go to previous / next guide comment"),
        bindings: &[
            ShortcutBinding::two(
                Key::Char('['),
                Key::Char('r'),
                guide(GuideShortcut::GoToPreviousComment),
            ),
            ShortcutBinding::two(
                Key::Char(']'),
                Key::Char('r'),
                guide(GuideShortcut::GoToNextComment),
            ),
        ],
    },
    ShortcutDefinition {
        description: Some("Go to previous / next modified hunk"),
        bindings: &[
            ShortcutBinding::two(
                Key::Char('['),
                Key::Char('h'),
                ShortcutCommand::Hunk(HunkShortcut::GoToPreviousModified),
            ),
            ShortcutBinding::two(
                Key::Char(']'),
                Key::Char('h'),
                ShortcutCommand::Hunk(HunkShortcut::GoToNextModified),
            ),
        ],
    },
    ShortcutDefinition {
        description: Some("Go to previous / next unreviewed file"),
        bindings: &[
            ShortcutBinding::two(
                Key::Char('['),
                Key::Char('f'),
                ShortcutCommand::File(FileShortcut::GoToPreviousUnreviewed),
            ),
            ShortcutBinding::two(
                Key::Char(']'),
                Key::Char('f'),
                ShortcutCommand::File(FileShortcut::GoToNextUnreviewed),
            ),
        ],
    },
    ShortcutDefinition {
        description: Some("Show the commit message"),
        bindings: &[
            ShortcutBinding::alias(
                Key::CommitMessage,
                application(ApplicationShortcut::ShowCommitMessage),
            ),
            ShortcutBinding::one(
                Key::Char('c'),
                application(ApplicationShortcut::ShowCommitMessage),
            ),
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
            application(ApplicationShortcut::OpenHelp),
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

pub fn lookup(prefix: Option<ShortcutPrefix>, key: Key) -> ShortcutLookup {
    let bindings = SHORTCUTS.iter().flat_map(|definition| definition.bindings);
    let mut has_prefix = false;
    for binding in bindings {
        if binding.sequence.matches(prefix, key) {
            return ShortcutLookup::Command(binding.command);
        }
        has_prefix |= prefix.is_none() && binding.sequence.starts_with(key);
    }
    if has_prefix {
        ShortcutLookup::Prefix(ShortcutPrefix::new(key))
    } else {
        ShortcutLookup::None
    }
}

const fn is_files_shortcut(command: ShortcutCommand) -> bool {
    matches!(
        command,
        ShortcutCommand::Navigation(
            NavigationShortcut::MoveDown
                | NavigationShortcut::MoveUp
                | NavigationShortcut::GoToFirst
                | NavigationShortcut::GoToLast
                | NavigationShortcut::MoveHalfPageDown
                | NavigationShortcut::MoveHalfPageUp
        )
    )
}

const fn is_revision_shortcut(command: ShortcutCommand) -> bool {
    matches!(
        command,
        ShortcutCommand::Navigation(
            NavigationShortcut::GoToParentRevision
                | NavigationShortcut::GoToChildRevision
                | NavigationShortcut::OpenRevisionSelector
        )
    )
}

const fn is_component_global_shortcut(command: ShortcutCommand) -> bool {
    matches!(
        command,
        ShortcutCommand::Comment(CommentShortcut::Previous | CommentShortcut::Next)
            | ShortcutCommand::Guide(_)
            | ShortcutCommand::File(_)
            | ShortcutCommand::Hunk(_)
            | ShortcutCommand::Navigation(
                NavigationShortcut::GoToParentRevision
                    | NavigationShortcut::GoToChildRevision
                    | NavigationShortcut::OpenRevisionSelector
            )
            | ShortcutCommand::Application(
                ApplicationShortcut::ShowCommitMessage
                    | ApplicationShortcut::OpenHelp
                    | ApplicationShortcut::MarkReviewed
            )
    )
}

/// Return the visible shortcut help lines.
pub fn help_lines() -> impl Iterator<Item = (String, &'static str)> {
    SHORTCUTS
        .iter()
        .filter_map(ShortcutDefinition::help_line)
        .chain([
            (
                "Up / Down / j / k (Explore)".into(),
                "Select an answer, including None of the above",
            ),
            (
                "Enter (Explore)".into(),
                "Send the selected answer with any additional text",
            ),
            (
                "Tab (Explore)".into(),
                "Focus conversation, inline evidence, then answer",
            ),
            (
                "Alt-j / Alt-k".into(),
                "Grow / shrink the Explore evidence window",
            ),
            (
                "Alt-0".into(),
                "Fit Explore evidence to its wrapped relevant range",
            ),
            (
                "[ / ] (Explore)".into(),
                "Visit previous / next interview turn",
            ),
        ])
}

/// Return the number of visible shortcut help lines.
pub fn help_line_count() -> usize {
    help_lines().count()
}

pub fn closes_help(key: Key) -> bool {
    SHORTCUTS
        .iter()
        .flat_map(|definition| definition.bindings)
        .any(|binding| binding.closes_help && binding.sequence == ShortcutSequence::One(key))
}

pub fn help_close_label() -> String {
    SHORTCUTS
        .iter()
        .flat_map(|definition| definition.bindings)
        .filter(|binding| binding.closes_help)
        .map(|binding| binding.sequence.label())
        .collect::<Vec<_>>()
        .join(" or ")
}

fn key_label(key: Key) -> String {
    match key {
        Key::Char(character) => return character.to_string(),
        Key::Control(character) => return format!("Ctrl-{character}"),
        Key::Alt(character) => return format!("Alt-{character}"),
        _ => {}
    }
    NAMED_KEY_LABELS
        .iter()
        .find_map(|(candidate, label)| (*candidate == key).then_some(*label))
        .expect("each non-character key must have a label")
        .to_owned()
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
