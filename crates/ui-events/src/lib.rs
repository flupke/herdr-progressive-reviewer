//! Typed events shared by review UI components.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Instant;

use review_guide::{GuideItem, GuideTarget, ReviewCheckpoint};
use review_lsp::{Operation, SourceLocation};
use review_repository::diff::DiffRow;
use review_repository::repository::ChangedFile;
use review_state::{ReviewState, ReviewStatus};
use review_types::ReviewUnit;
use toasts::ToastKind;

/// Repository text shown outside the file and diff panes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryMetadataChanged {
    pub review_checkpoint: ReviewCheckpoint,
    pub description: String,
}

/// Aggregate values shown in the repository header.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FilesOverviewChanged {
    pub reviewed: usize,
    pub total: usize,
    pub lines_added: u64,
    pub lines_removed: u64,
}

/// Search text shown in the status line.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SearchStatusChanged {
    pub query: Option<String>,
    pub current_match: usize,
    pub total_matches: usize,
}

/// Selection and viewport state for one language-server result list.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocationSelection {
    pub operation: Operation,
    pub locations: Vec<SourceLocation>,
    pub selected: usize,
    pub scroll: usize,
}

/// One pointer position in terminal and component coordinates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PointerPosition {
    pub terminal_column: u16,
    pub terminal_row: u16,
    pub component_column: u16,
    pub component_row: u16,
}

/// One normalized pointer operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PointerInputKind {
    Scroll(isize),
    Click { insert: bool },
    ControlClick,
    DoubleClick,
    RightClick,
    Drag,
    Release,
}

/// Pointer input routed to the component under the pointer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PointerInput {
    pub kind: PointerInputKind,
    pub position: Option<PointerPosition>,
}

/// Ask the overlay host to toggle the commit message.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommitMessageToggleRequested;

/// The current terminal dimensions used to place modal overlays.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ViewportChanged {
    pub width: u16,
    pub height: u16,
}

/// The drawable diff-pane dimensions used for cursor visibility.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DiffViewportChanged {
    pub width: u16,
    pub height: u16,
}

/// Remove transient diff input state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DiffInputClearRequested;

/// A selected output operation finished.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputDeliveryFinished {
    pub delivered: bool,
}

/// Complete source content loaded for one LSP location.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceContentLoaded {
    pub snapshot_id: String,
    pub location: SourceLocation,
    pub content: Vec<u8>,
    pub mode: SourceLoadMode,
}

/// A source-content load failed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceContentLoadFailed {
    pub snapshot_id: String,
    pub message: String,
}

/// How loaded source is presented and whether it becomes the current file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceLoadMode {
    Preview,
    External,
}

impl SourceLoadMode {
    pub fn is_external(self) -> bool {
        self == Self::External
    }
}

/// One source position without its operation-specific toast identifier.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LspQueryContext {
    pub path: PathBuf,
    pub line: u32,
    pub byte_column: usize,
    pub expected_line: String,
    pub snapshot_id: String,
}

/// Ask the overlay host to start one LSP operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LspQueryRequested {
    pub operation: review_lsp::Operation,
    pub query: LspQueryContext,
}

/// Ask the overlay host to open the source context menu.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextMenuRequested {
    pub column: u16,
    pub row: u16,
    pub query: Option<LspQueryContext>,
}

/// Ask the diff component to preview one LSP result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceLocationPreviewRequested {
    pub location: SourceLocation,
}

/// Ask the diff component to accept one LSP result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceLocationAccepted {
    pub location: SourceLocation,
}

/// The LSP location-list overlay became active or inactive.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocationListVisibilityChanged {
    pub visible: bool,
}

/// Add one short application notification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToastRequested {
    pub text: String,
    pub kind: ToastKind,
}

/// Expire timed application notifications at this instant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToastExpirationTick {
    pub now: Instant,
}

/// One stable row identity inside a diff presentation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PresentationLocation {
    Context { old_line: u32, new_line: u32 },
    NewLine(u32),
    OldLine(u32),
    SourceRow(usize),
    GapStart(u32),
}

/// One restorable location in a review revision or source file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReviewLocation {
    LoadedDocument {
        review_unit: ReviewUnit,
        path: String,
        cursor: usize,
        presentation_location: Option<PresentationLocation>,
        column: usize,
    },
    Source {
        review_unit: ReviewUnit,
        location: SourceLocation,
    },
    Revision {
        review_unit: ReviewUnit,
    },
}

impl ReviewLocation {
    pub fn review_unit(&self) -> &ReviewUnit {
        match self {
            Self::LoadedDocument { review_unit, .. }
            | Self::Source { review_unit, .. }
            | Self::Revision { review_unit } => review_unit,
        }
    }

    #[must_use]
    pub fn for_review_unit(&self, review_unit: impl Into<ReviewUnit>) -> Self {
        let review_unit = review_unit.into();
        match self {
            Self::LoadedDocument {
                path,
                cursor,
                presentation_location,
                column,
                ..
            } => Self::LoadedDocument {
                review_unit,
                path: path.clone(),
                cursor: *cursor,
                presentation_location: *presentation_location,
                column: *column,
            },
            Self::Source { location, .. } => Self::Source {
                review_unit,
                location: location.clone(),
            },
            Self::Revision { .. } => Self::Revision { review_unit },
        }
    }

    pub fn same_line(&self, other: &Self) -> bool {
        match self {
            Self::LoadedDocument { .. } => self.same_loaded_document_line(other),
            Self::Source { .. } => self.same_source_line(other),
            Self::Revision { review_unit } => matches!(
                other,
                Self::Revision {
                    review_unit: other_review_unit
                } if review_unit == other_review_unit
            ),
        }
    }

    fn same_loaded_document_line(&self, other: &Self) -> bool {
        let Self::LoadedDocument {
            review_unit,
            path,
            cursor,
            presentation_location,
            ..
        } = self
        else {
            unreachable!("loaded-document comparison starts from a loaded document");
        };
        let Self::LoadedDocument {
            review_unit: other_review_unit,
            path: other_path,
            cursor: other_cursor,
            presentation_location: other_presentation_location,
            ..
        } = other
        else {
            return false;
        };
        review_unit == other_review_unit
            && path == other_path
            && same_presentation_line(
                *presentation_location,
                *other_presentation_location,
                *cursor,
                *other_cursor,
            )
    }

    fn same_source_line(&self, other: &Self) -> bool {
        let Self::Source {
            review_unit,
            location,
        } = self
        else {
            unreachable!("source comparison starts from a source location");
        };
        let Self::Source {
            review_unit: other_review_unit,
            location: other_location,
        } = other
        else {
            return false;
        };
        review_unit == other_review_unit
            && location.path == other_location.path
            && location.line == other_location.line
    }
}

fn same_presentation_line(
    location: Option<PresentationLocation>,
    other_location: Option<PresentationLocation>,
    cursor: usize,
    other_cursor: usize,
) -> bool {
    match (location, other_location) {
        (Some(location), Some(other_location)) => location == other_location,
        (None, None) => cursor == other_cursor,
        (Some(_), None) | (None, Some(_)) => false,
    }
}

/// The current location used as a revision-navigation destination shape.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CurrentReviewLocationChanged {
    pub location: Option<ReviewLocation>,
}

/// One semantic navigation jump that can be restored through location history.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewLocationJumped {
    pub origin: ReviewLocation,
    pub target: ReviewLocation,
}

/// Ask the diff component to restore one location after a revision edit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewLocationRestoreRequested {
    pub location: ReviewLocation,
}

/// A repository refresh started.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RepositoryRefreshStarted;

/// A repository refresh finished without a new snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RepositoryRefreshFinished;

/// Revision candidates returned from repository work.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RevisionCandidatesLoaded {
    pub direction: review_repository::repository::RevisionDirection,
    pub result: Result<Vec<review_repository::repository::RevisionCandidate>, String>,
}

/// Identity of one revision-history load requested by the UI.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RevisionHistoryLoadId(u64);

impl RevisionHistoryLoadId {
    /// Create an identity from the component-local sequence number.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
}

/// Terminal-rendered revision history returned from repository work.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RevisionHistoryLoaded {
    pub load_id: RevisionHistoryLoadId,
    pub result: Result<Vec<review_repository::repository::RevisionHistoryLine>, String>,
}

/// A requested revision edit did not produce a new repository snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RevisionEditFailed {
    pub message: Option<String>,
}

/// Repository metadata shown in the files pane.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileSummary {
    pub file: ChangedFile,
    pub review_state: ReviewState,
    pub temporary: bool,
    pub disk_path: Option<PathBuf>,
}

impl FileSummary {
    /// Create one file summary before its repository metadata arrives.
    pub fn new(path: impl Into<String>, status: ReviewStatus) -> Self {
        Self::from_changed(&ChangedFile::modified(path.into()), status)
    }

    /// Create a file summary from repository metadata.
    pub fn from_changed(file: &ChangedFile, status: ReviewStatus) -> Self {
        let state = match status {
            ReviewStatus::Unreviewed => ReviewState::unreviewed(file.statistics, None),
            ReviewStatus::Reviewed => ReviewState::reviewed(),
            ReviewStatus::ChangedSinceReview => ReviewState::changed_since_review(file.statistics),
        };
        Self::from_review_state(file, state)
    }

    /// Create a file summary with derived checkpoint-relative state.
    pub fn from_review_state(file: &ChangedFile, state: ReviewState) -> Self {
        Self {
            file: file.clone(),
            review_state: state,
            temporary: false,
            disk_path: None,
        }
    }

    /// Create one temporary source entry for navigation outside the review diff.
    pub fn temporary(
        path: impl Into<String>,
        display_path: impl Into<String>,
        disk_path: PathBuf,
    ) -> Self {
        let path = path.into();
        let mut file = ChangedFile::modified(&path);
        file.display_path = display_path.into();
        Self {
            file,
            review_state: ReviewState::reviewed(),
            temporary: true,
            disk_path: Some(disk_path),
        }
    }

    /// Return the repository path used for review state.
    pub fn path(&self) -> String {
        self.file.review_path().display()
    }

    /// Return the escaped path shown in the files pane.
    pub fn display_path(&self) -> &str {
        &self.file.display_path
    }
}

/// The current set of files that still need review.
///
/// The files component is the only production writer. Other components use
/// clones of this handle for read-only review and navigation decisions. Before
/// the first file list arrives, the handle permits paths so mounted consumers
/// can initialize in any order.
#[derive(Debug, Default)]
struct ReviewableFilePaths {
    initialized: bool,
    paths: HashSet<String>,
}

#[derive(Clone, Debug, Default)]
pub struct ReviewableFiles(Arc<RwLock<ReviewableFilePaths>>);

impl ReviewableFiles {
    /// Replace the current reviewable paths.
    pub fn replace(&self, paths: HashSet<String>) -> bool {
        let mut current = self
            .0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if current.initialized && current.paths == paths {
            return false;
        }
        current.initialized = true;
        current.paths = paths;
        true
    }

    /// Test if one path still needs review.
    pub fn contains(&self, path: &str) -> bool {
        let current = self
            .0
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        !current.initialized || current.paths.contains(path)
    }
}

/// The files that need review changed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReviewableFilesChanged;

/// A complete repository snapshot for the files component.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryFilesChanged {
    pub review_checkpoint: ReviewCheckpoint,
    pub files: Vec<FileSummary>,
}

/// Temporary source files that the diff component currently displays.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TemporaryFilesChanged {
    pub files: Vec<FileSummary>,
}

/// A completed review-state write for one path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewStateSaved {
    pub review_unit: ReviewUnit,
    pub path: String,
    pub result: Result<ReviewState, ()>,
}

/// Paths that currently have visible guide comments.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GuidePathsChanged {
    pub paths: Vec<String>,
}

/// Guide generation state for one exact repository checkpoint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewGuideStatusChanged {
    pub review_checkpoint: ReviewCheckpoint,
    pub generating: bool,
    pub message: Option<String>,
}

/// One complete guide for an exact repository checkpoint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewGuideChanged {
    pub review_checkpoint: ReviewCheckpoint,
    pub items: Vec<GuideItem>,
}

/// The position of one visible guide item in the complete guide.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GuideCounter {
    pub number: usize,
    pub total: usize,
}

/// The guide data needed to lay out comments in the displayed diff.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GuideLayoutChanged {
    pub items: Vec<GuideItem>,
    pub counters: Vec<Option<GuideCounter>>,
}

/// One presented diff row that can anchor a guide target.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DisplayedDiffRow {
    pub hunk: Option<usize>,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    pub changed: bool,
}

/// The guide-relevant shape of one displayed diff document.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DisplayedDiffViewport {
    pub path: String,
    pub file_index: usize,
    pub rows: Vec<DisplayedDiffRow>,
    pub can_show_file: bool,
}

/// Guide-relevant diff viewports and the current navigation position.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DisplayedDiffViewportsChanged {
    pub viewports: Vec<DisplayedDiffViewport>,
    pub current_file_index: usize,
    pub current_row: usize,
}

/// A request to show one guide target at its current diff position.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuideJumpRequested {
    pub file_index: usize,
    pub row: Option<usize>,
    pub target: GuideTarget,
}

/// One animation tick for visible components.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AnimationTick;

/// Diff-owned decorations that affect file-list rendering.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FileDecorationsChanged {
    pub notice_paths: Vec<String>,
    pub search_match_paths: Vec<String>,
}

/// The number of rows available inside the files pane.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FilesViewportChanged {
    pub rows: usize,
}

/// A request from another component to display one repository path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileSelectionRequested {
    pub path: String,
}

/// The file that the files component selected.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileSelected {
    pub path: String,
}

/// Loaded diff content for one repository checkpoint and path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiffContentLoaded {
    pub review_checkpoint: ReviewCheckpoint,
    pub path: String,
    pub rows: Vec<DiffRow>,
    pub old_content: Option<Vec<u8>>,
    pub new_content: Option<Vec<u8>>,
}

/// A failed diff load for one repository checkpoint and path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiffContentLoadFailed {
    pub review_checkpoint: ReviewCheckpoint,
    pub path: String,
}
