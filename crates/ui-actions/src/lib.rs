//! External work requested by review UI components.

use review_guide::{GuideScope, ReviewCheckpoint};
use review_lsp::{Operation, Query, SourceLocation};
use review_repository::repository::{ChangeId, RevisionDirection};

pub use ui_events::{RevisionHistoryLoadId, SourceLoadMode};

/// Work that the I/O layer must perform after a UI update.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Action {
    /// Load one path diff for the exact current snapshot.
    LoadDiff {
        review_checkpoint: ReviewCheckpoint,
        path: String,
    },
    /// Load every unopened path diff for the exact current snapshot.
    LoadDiffs {
        review_checkpoint: ReviewCheckpoint,
        paths: Vec<String>,
    },
    /// Find mutable jj commits next to the working-copy commit.
    LoadRevisionCandidates(RevisionDirection),
    /// Render mutable jj history through its first immutable parent.
    LoadRevisionHistory { load_id: RevisionHistoryLoadId },
    /// Make one jj change the working-copy commit.
    EditRevision { change_id: ChangeId },
    /// Run one LSP request at a visible disk position.
    Lsp { operation: Operation, query: Query },
    /// Restart the language server.
    RestartLsp,
    /// Load complete disk source for a target location.
    LoadSource {
        snapshot_id: String,
        location: SourceLocation,
        mode: SourceLoadMode,
    },
    /// Set the selected path review state.
    SetReviewed { path: String, reviewed: bool },
    /// Send selected text to the active implementation agent.
    Output { text: String },
    /// Generate a guide through the active implementation agent.
    GenerateReviewGuide { scope: GuideScope },
    /// Save the file-pane width in terminal columns.
    SaveFilePaneWidth(u16),
    /// Stop the application.
    Quit,
}
