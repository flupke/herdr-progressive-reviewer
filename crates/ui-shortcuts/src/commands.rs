//! Keyboard commands grouped by the scope that owns them.
//!
//! Each [`ShortcutCommand`] variant is one owning scope. A command belongs to
//! exactly one scope because it is a variant of exactly one owner enum, so the
//! table entry that builds the command also declares its owner.

use crate::Key;
use crate::table::{ShortcutSequence, bindings};

/// One command in the shortcut table, tagged with the scope that owns it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShortcutCommand {
    Application(ApplicationShortcut),
    Search(SearchShortcut),
    Movement(MovementShortcut),
    Diff(DiffShortcut),
    DiffGlobal(DiffGlobalShortcut),
    Files(FilesShortcut),
    Overlay(OverlayShortcut),
    Revision(RevisionShortcut),
    Threads(ThreadsShortcut),
    Conversation(ConversationShortcut),
    Explore(ExploreShortcut),
    ExploreGlobal(ExploreGlobalShortcut),
}

/// Commands the application runs when no focused component handled a key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplicationShortcut {
    ChangeFocus,
    OpenFiles,
    OpenThreads,
    OpenExplore,
    ToggleNavigation,
    NewReplies,
    Clear,
    Quit,
}

/// Starting a search, shared by the application and the focused diff.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SearchShortcut {
    Begin,
}

/// Cursor movement shared by every list and source view.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MovementShortcut {
    MoveDown,
    MoveUp,
    GoToFirst,
    GoToLast,
    MoveHalfPageDown,
    MoveHalfPageUp,
}

impl MovementShortcut {
    /// The signed number of rows this move covers in a view that pages by
    /// `page` rows. Going to the first or last row saturates.
    pub fn row_delta(self, page: isize) -> isize {
        match self {
            Self::MoveDown => 1,
            Self::MoveUp => -1,
            Self::MoveHalfPageDown => page,
            Self::MoveHalfPageUp => -page,
            Self::GoToFirst => isize::MIN,
            Self::GoToLast => isize::MAX,
        }
    }
}

/// Commands the diff runs while it has focus.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiffShortcut {
    Comment(CommentShortcut),
    Location(LocationShortcut),
    SearchMatch(SearchMatchShortcut),
    Source(SourceShortcut),
    Lsp(LspShortcut),
    StartSelection,
}

/// Commands the diff runs whichever pane has focus.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiffGlobalShortcut {
    PreviousComment,
    NextComment,
    Hunk(HunkShortcut),
    ToggleHunkReviewed,
    OpenInEditor,
}

/// Commands the file list runs whichever pane has focus.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FilesShortcut {
    GoToNextUnreviewed,
    GoToPreviousUnreviewed,
    MarkReviewed,
    AutoReview,
    UnreviewAll,
}

/// Commands that open an overlay.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OverlayShortcut {
    ShowCommitMessage,
    OpenHelp,
}

/// Commands that change the reviewed revisions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RevisionShortcut {
    GoToParent,
    GoToChild,
    OpenSelector,
}

/// Commands the Threads list runs while it has focus.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThreadsShortcut {
    ShowUnresolved,
    ShowAll,
    OpenConversation,
}

/// Commands the thread conversation view runs while it has focus.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConversationShortcut {
    Reply,
    ToggleResolution,
    Peek,
    MarkRead,
    FocusThreads,
    /// Close the source peek, or return to Files when nothing is peeked.
    Back,
}

/// Commands the Explore conversation runs while it has focus.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExploreShortcut {
    /// Select the next answer, or scroll down when no answer is selectable.
    SelectNext,
    /// Select the previous answer, or scroll up when no answer is selectable.
    SelectPrevious,
    ScrollDown,
    ScrollUp,
    /// Send the selected answer, or start replying to the current turn.
    Confirm,
    /// Send the composed answer or implementation instructions.
    Send,
    /// Leave the evidence list or close the conclusion preview.
    Back,
    /// Select one answer by its zero-based position.
    ChooseAnswer(usize),
    Turn(ExploreTurnShortcut),
    Evidence(ExploreEvidenceShortcut),
}

/// Explore commands that act on the interview turn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExploreTurnShortcut {
    Start,
    Cancel,
    Retry,
    PreviousTurn,
    NextTurn,
    ToggleMap,
}

/// Explore commands that choose which evidence the conversation shows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExploreEvidenceShortcut {
    /// Return to the turn's first, most decisive evidence.
    First,
    /// Show the turn's next evidence.
    Next,
}

/// Commands Explore runs whether its conversation or its evidence has focus.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExploreGlobalShortcut {
    /// Move focus through the conversation, the inline evidence and the answer.
    CycleFocus,
    GrowEvidence,
    ShrinkEvidence,
    /// Fit the evidence window to its wrapped relevant range.
    FitEvidence,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommentShortcut {
    Add,
    Reply,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocationShortcut {
    GoToPrevious,
    GoToNext,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SearchMatchShortcut {
    WordUnderCursor,
    NextMatch,
    PreviousMatch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HunkShortcut {
    GoToNextModified,
    GoToPreviousModified,
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
pub enum SourceShortcut {
    ExpandOrMoveRight,
    MoveLeft,
    MoveToEndOfLine,
    MoveToNextWord,
    MoveToPreviousWord,
    MoveToStartOfLine,
}

/// The commands one input subscription resolves.
///
/// Every owner enum is a subscription to its own scope. A component that
/// listens to several scopes subscribes through an enum that combines them.
pub trait ShortcutSubscription: Copy + Eq + Send + Sync + 'static {
    /// Return this subscription's command, or `None` when it does not listen
    /// to the scope that owns `command`.
    fn select(command: ShortcutCommand) -> Option<Self>;

    /// Return the command `key` runs on its own in this subscription's scopes.
    ///
    /// Components use it to recognize a key that belongs to another scope,
    /// such as an application command they must let through, without naming
    /// the key. Two-key sequences are ignored.
    fn bound_to(key: Key) -> Option<Self> {
        bindings()
            .filter(|binding| binding.sequence == ShortcutSequence::One(key))
            .find_map(|binding| Self::select(binding.command))
    }

    /// Every key that runs this command on its own.
    fn keys(self) -> impl Iterator<Item = Key> {
        bindings().filter_map(move |binding| match binding.sequence {
            ShortcutSequence::One(key) if Self::bound_to(key) == Some(self) => Some(key),
            _ => None,
        })
    }
}

macro_rules! owner_subscription {
    ($owner:ident, $scope:ident) => {
        impl ShortcutSubscription for $owner {
            fn select(command: ShortcutCommand) -> Option<Self> {
                match command {
                    ShortcutCommand::$scope(command) => Some(command),
                    _ => None,
                }
            }
        }
    };
}

owner_subscription!(ApplicationShortcut, Application);
owner_subscription!(SearchShortcut, Search);
owner_subscription!(MovementShortcut, Movement);
owner_subscription!(DiffShortcut, Diff);
owner_subscription!(DiffGlobalShortcut, DiffGlobal);
owner_subscription!(FilesShortcut, Files);
owner_subscription!(OverlayShortcut, Overlay);
owner_subscription!(RevisionShortcut, Revision);
owner_subscription!(ThreadsShortcut, Threads);
owner_subscription!(ConversationShortcut, Conversation);
owner_subscription!(ExploreShortcut, Explore);
owner_subscription!(ExploreGlobalShortcut, ExploreGlobal);

/// The scopes the application resolves after focused components pass on a key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplicationCommand {
    Application(ApplicationShortcut),
    Search(SearchShortcut),
}

impl ShortcutSubscription for ApplicationCommand {
    fn select(command: ShortcutCommand) -> Option<Self> {
        ApplicationShortcut::select(command)
            .map(Self::Application)
            .or_else(|| SearchShortcut::select(command).map(Self::Search))
    }
}

/// The scopes the diff resolves while it has focus.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiffPaneCommand {
    Movement(MovementShortcut),
    Search(SearchShortcut),
    Diff(DiffShortcut),
}

impl ShortcutSubscription for DiffPaneCommand {
    fn select(command: ShortcutCommand) -> Option<Self> {
        MovementShortcut::select(command)
            .map(Self::Movement)
            .or_else(|| SearchShortcut::select(command).map(Self::Search))
            .or_else(|| DiffShortcut::select(command).map(Self::Diff))
    }
}

/// The scopes the Threads list resolves while it has focus.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThreadsCommand {
    Movement(MovementShortcut),
    Search(SearchShortcut),
    Threads(ThreadsShortcut),
}

impl ShortcutSubscription for ThreadsCommand {
    fn select(command: ShortcutCommand) -> Option<Self> {
        MovementShortcut::select(command)
            .map(Self::Movement)
            .or_else(|| SearchShortcut::select(command).map(Self::Search))
            .or_else(|| ThreadsShortcut::select(command).map(Self::Threads))
    }
}

/// The scopes the thread conversation view resolves while it has focus.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConversationCommand {
    Movement(MovementShortcut),
    Conversation(ConversationShortcut),
}

impl ShortcutSubscription for ConversationCommand {
    fn select(command: ShortcutCommand) -> Option<Self> {
        MovementShortcut::select(command)
            .map(Self::Movement)
            .or_else(|| ConversationShortcut::select(command).map(Self::Conversation))
    }
}

/// The scopes the Explore conversation resolves while it has focus.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExploreCommand {
    Global(ExploreGlobalShortcut),
    Explore(ExploreShortcut),
}

impl ShortcutSubscription for ExploreCommand {
    fn select(command: ShortcutCommand) -> Option<Self> {
        ExploreGlobalShortcut::select(command)
            .map(Self::Global)
            .or_else(|| ExploreShortcut::select(command).map(Self::Explore))
    }
}
