//! External work requested by review UI components.
//!
//! Each variant of [`Action`] is one runtime executor's group, so the runtime
//! routes with one exhaustive match and each executor matches only its own group.
//! The document executor feeds the document worker, highlighter, search and
//! source watcher; the document worker itself receives only [`DocumentLoad`].

use std::path::PathBuf;

use review_lsp::{Operation, Query, SourceLocation};
use review_repository::repository::{ChangeId, RevisionDirection};
use review_source::ReviewCheckpoint;

pub use ui_events::{RevisionHistoryLoadId, SourceLoadMode};

/// Work that the I/O layer must perform after a UI update.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Action {
    Explore(review_explore::Command),
    /// Show the Explore page the pane serves.
    ExplorePage(ExplorePageAction),
    /// Load or update a review conversation through its serial owner.
    Thread(review_threads::ThreadCommand),
    /// Load, color, search or watch the documents on screen.
    Document(DocumentAction),
    /// Talk to the language server.
    Lsp(LspAction),
    /// Save a reviewer setting.
    Settings(SettingsAction),
    /// Read or change the repository and its review marks.
    Repository(RepositoryAction),
    /// Work that needs the terminal the UI runs in.
    Terminal(TerminalAction),
}

/// Work on the Explore page the pane serves.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExplorePageAction {
    /// Open the page in the default browser.
    Open,
    /// Share the running round over a tunnel, until the round ends.
    OpenTunnel,
    /// Stop the tunnel that shares the running round.
    CloseTunnel,
}

/// Work on the documents on screen.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DocumentAction {
    /// Read diff or source content for display.
    Load(DocumentLoad),
    /// Color loaded text without delaying input or search.
    Highlight(ui_events::HighlightRequest),
    /// Search immutable presented text, or cancel the previous search.
    Search(Option<text_search::Request>),
    /// Watch the active live source, including ignored paths; None closes the watch.
    WatchSource(Option<PathBuf>),
}

/// Content reads run by the document worker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DocumentLoad {
    /// Load one path diff for the exact current snapshot.
    Diff {
        review_checkpoint: ReviewCheckpoint,
        path: String,
    },
    /// Load every unopened path diff for the exact current snapshot.
    Diffs {
        review_checkpoint: ReviewCheckpoint,
        paths: Vec<String>,
    },
    /// Load complete disk source for a target location.
    Source {
        snapshot_id: String,
        location: SourceLocation,
        mode: SourceLoadMode,
    },
}

/// Language server work.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LspAction {
    /// Notify the language server when a document is selected.
    OpenDocument(PathBuf),
    /// Run one LSP request at a visible disk position.
    Request { operation: Operation, query: Query },
    /// Restart the language server.
    Restart,
}

/// Settings saved for the next reviewer session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettingsAction {
    /// Save the file-pane width in terminal columns.
    SaveFilePaneWidth(u16),
    /// Save the keymap shared by every text editor.
    SaveEditorKeymap(review_types::EditorKeymap),
    /// Save one setting of the Explore page, which takes effect at once.
    SaveExplorePage(review_explore_page_settings::ExplorePageSetting),
    /// Save the writing style of the next Explore round.
    SaveExploreWritingStyle(review_explore_round_settings::WritingStyle),
    /// Save which choices run-ahead prepares, from the next question on.
    SaveExploreRunAhead(review_explore_round_settings::RunAhead),
}

/// Repository and review-mark work run by the repository worker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RepositoryAction {
    /// Find mutable jj commits next to the working-copy commit.
    LoadRevisionCandidates(RevisionDirection),
    /// Render mutable jj history through its first immutable parent.
    LoadRevisionHistory { load_id: RevisionHistoryLoadId },
    /// Make one jj change the working-copy commit.
    EditRevision { change_id: ChangeId },
    /// Set the selected path review state.
    SetReviewed { path: String, reviewed: bool },
    /// Accept or reopen one hunk of a path, as the exact comparison showed it.
    SetHunkReviewed {
        review_checkpoint: ReviewCheckpoint,
        path: String,
        mark: review_hunks::HunkMark,
    },
    /// Classify this comparison and mark files containing only insignificant changes.
    AutoReview(ReviewCheckpoint),
    /// Clear this comparison's file review marks after user confirmation.
    UnreviewAll(ReviewCheckpoint),
}

/// Work run by the terminal owner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TerminalAction {
    /// Suspend the UI and open a file in the user's editor at a zero-based line.
    OpenInEditor { path: PathBuf, line: Option<u32> },
    /// Stop the application.
    Quit,
}
