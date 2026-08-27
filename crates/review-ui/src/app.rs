//! Pure review state and navigation.

use std::collections::HashSet;
use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::time::Instant;

use ratatui::layout::Rect;
use review_guide::{GuideItem, GuideScope, GuideTarget, ReviewCheckpoint};
use review_lsp::{Event, Operation, Query, SourceLocation};
use review_repository::diff::DiffRow;
use review_repository::repository::{ChangeKind, ChangedFile};
use review_state::{ReviewState, ReviewStatus};
use review_store::OutputTarget;
use toasts::{ToastId, ToastKind, ToastState};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::diff::{DiffView, GuideTargetPosition, TAB_DISPLAY_WIDTH};
use crate::file_tree::FileTree;
use crate::highlight::SyntaxHighlighter;
use crate::navigation::{LocationHistory, ReviewLocation};
use crate::presentation::{DiffPresentation, PresentationLocation, SearchDirection};
use crate::theme::{Palette, Theme};

const NARROW_WIDTH: u16 = 72;
pub(super) const MIN_PANE_WIDTH: u16 = 16;
const DIFF_CONTROLS_TITLE: &str = "[←→] [→←] [👁 ]";
const BASIC_DIFF_CONTROLS_TITLE: &str = "[←→] [→←]";
const FILE_CONTROL_TITLE: &str = "[x]";
const EXPAND_ALL_LABEL: &str = "[←→]";
const CONTRACT_ALL_LABEL: &str = "[→←]";
const MIN_DIFF_CONTROLS_WIDTH: u16 = 32;
use crate::review_view::ReviewView;

/// The pane that receives navigation keys.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Focus {
    /// The changed-file list.
    Files,
    /// The selected file diff.
    Diff,
}

/// How loaded source is presented and whether it becomes the current file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceLoadMode {
    /// Show the source only as a result-list preview.
    Preview,
    /// Select the source with flat external-file presentation.
    External,
}

impl SourceLoadMode {
    pub(super) fn external(self) -> bool {
        self == Self::External
    }
}

/// One normalized keyboard input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Key {
    Char(char),
    Backspace,
    Tab,
    Down,
    Up,
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
    Space,
    Quit,
}

/// One file presented by the review state machine.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewFile {
    /// Escaped repository-relative path used by review operations.
    pub path: String,
    pub(super) review_path: Option<String>,
    /// Escaped text shown to the user.
    pub(super) display_path: String,
    /// Current review state.
    pub status: ReviewStatus,
    pub(super) change: ChangeKind,
    pub(super) lines_added: u64,
    pub(super) lines_removed: u64,
    pub(super) diff: DiffPresentation,
    pub(super) cursor: usize,
    pub(super) scroll: usize,
    pub(super) column: usize,
    pub(super) source_location: Option<SourceLocation>,
    pub(super) loading: bool,
    pub(super) temporary: bool,
    pub(super) disk_path: Option<PathBuf>,
}

impl ReviewFile {
    /// Create one file summary before its diff arrives.
    pub fn new(path: impl Into<String>, status: ReviewStatus) -> Self {
        let path = path.into();
        Self {
            display_path: path.clone(),
            review_path: Some(path.clone()),
            path,
            status,
            change: ChangeKind::Modified,
            lines_added: 0,
            lines_removed: 0,
            diff: DiffPresentation::default(),
            cursor: 0,
            scroll: 0,
            column: 0,
            source_location: None,
            loading: false,
            temporary: false,
            disk_path: None,
        }
    }

    pub(super) fn from_source(
        location: &SourceLocation,
        review_path: Option<&str>,
        diff: DiffPresentation,
        mode: SourceLoadMode,
    ) -> Self {
        let external = mode.external();
        let tree_path = if external {
            location.path.file_name().map_or_else(
                || location.path.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            )
        } else {
            review_path.map_or_else(|| location.path.display().to_string(), str::to_owned)
        };
        let mut file = Self::new(tree_path, ReviewStatus::Unreviewed);
        file.display_path = if external {
            location.path.display().to_string()
        } else {
            review_path.map_or_else(|| location.path.display().to_string(), str::to_owned)
        };
        file.diff = diff;
        file.temporary = true;
        file.disk_path = Some(location.path.clone());
        file.review_path = review_path.map(str::to_owned);
        file
    }

    /// Create a file summary from repository metadata.
    pub fn from_changed(file: &ChangedFile, status: ReviewStatus) -> Self {
        Self {
            display_path: file.display_path.clone(),
            change: file.change,
            lines_added: file.lines_added,
            lines_removed: file.lines_removed,
            ..Self::new(file.review_path().display(), status)
        }
    }

    pub(super) fn has_notice(&self) -> bool {
        self.diff.has_notice()
    }

    pub(super) fn reveal_location(&mut self, location: &SourceLocation, page: usize) -> bool {
        self.column = location.byte_column;
        self.source_location = Some(location.clone());
        let row = self.diff.reveal_line(location.line);
        let Some(row) = row else {
            return false;
        };
        self.jump_to_row(row, page);
        true
    }

    pub(super) fn restore_presentation_location(
        &mut self,
        location: Option<PresentationLocation>,
        fallback_row: usize,
        column: usize,
    ) {
        let row = location
            .and_then(|location| self.diff.reveal_presentation_location(location))
            .unwrap_or_else(|| fallback_row.min(self.diff.len().saturating_sub(1)));
        self.column = column;
        self.clear_source_location();
        self.cursor = row;
    }

    pub(super) fn jump_to_row(&mut self, row: usize, page: usize) {
        self.cursor = row;
        self.scroll = self.cursor.saturating_sub(page / 2);
    }

    pub(super) fn cursor_location(&self) -> Option<SourceLocation> {
        self.diff.source_position(self.cursor).map_or_else(
            || self.source_location.clone(),
            |(line, _)| {
                let byte_column = self.column;
                Some(SourceLocation {
                    path: self
                        .disk_path
                        .clone()
                        .unwrap_or_else(|| PathBuf::from(&self.path)),
                    line,
                    byte_column,
                    end_line: line,
                    end_byte_column: byte_column,
                })
            },
        )
    }

    pub(super) fn marker(&self) -> &'static str {
        if self.has_notice() {
            "!"
        } else {
            match self.status {
                ReviewStatus::Unreviewed => "○",
                ReviewStatus::Reviewed => "✓",
                ReviewStatus::ChangedSinceReview => "●",
            }
        }
    }

    pub(super) fn start_diff_load(&mut self) -> Option<String> {
        if !self.diff.is_empty() || self.diff.can_show_file() || self.loading {
            return None;
        }
        self.loading = true;
        Some(self.path.clone())
    }

    pub(super) fn clear_source_location(&mut self) {
        self.source_location = None;
    }
}

fn needs_parent_expansion(status: ReviewStatus, previous: ReviewStatus) -> bool {
    status != previous && status != ReviewStatus::Reviewed
}

/// A typed result from keyboard input or asynchronous work.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Message {
    /// A complete repository poll.
    FilesLoaded {
        change_id: String,
        commit_id: String,
        description: String,
        files: Vec<ReviewFile>,
    },
    /// A diff result for an exact change and path.
    DiffLoaded {
        commit_id: String,
        path: String,
        rows: Vec<DiffRow>,
        old_content: Option<Vec<u8>>,
        new_content: Option<Vec<u8>>,
    },
    /// A diff command failed.
    DiffFailed { commit_id: String, path: String },
    /// A review-state write result.
    ReviewFinished {
        change_id: String,
        path: String,
        result: Result<ReviewState, ()>,
    },
    /// Whether selected text reached its output target.
    OutputFinished { delivered: bool },
    /// Whether an exact guide request is generating.
    ReviewGuideStatus {
        review_checkpoint: ReviewCheckpoint,
        generating: bool,
        message: Option<String>,
    },
    /// One complete guide for an exact repository checkpoint.
    ReviewGuideLoaded {
        review_checkpoint: ReviewCheckpoint,
        items: Vec<GuideItem>,
    },
    /// One language-server event.
    Lsp(Event),
    /// Complete disk source arrived for navigation or preview.
    SourceLoaded {
        snapshot_id: String,
        location: SourceLocation,
        content: Vec<u8>,
        mode: SourceLoadMode,
    },
    /// A disk source read failed.
    SourceFailed {
        snapshot_id: String,
        message: String,
    },
    /// Advance timed UI state.
    Tick(Instant),
    /// The terminal size changed.
    Resize { width: u16, height: u16 },
    /// The mouse wheel moved over one pane.
    MouseScroll { column: u16, row: u16, delta: isize },
    /// A mouse button was pressed over one pane.
    MouseClick {
        column: u16,
        row: u16,
        insert_path: bool,
    },
    /// The left mouse button was pressed with Control held.
    MouseControlClick { column: u16, row: u16 },
    /// The left mouse button was pressed twice at one position.
    MouseDoubleClick { column: u16, row: u16 },
    /// The right mouse button was pressed.
    MouseRightClick { column: u16, row: u16 },
    /// The left mouse button dragged over the pane.
    MouseDrag { column: u16, row: u16 },
    /// The left mouse button was released.
    MouseRelease,
    /// One keyboard input.
    Key(Key),
}

/// Work that the I/O layer must perform after an update.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Action {
    /// No external work is required.
    None,
    /// Load one path diff for the exact current snapshot.
    LoadDiff { commit_id: String, path: String },
    /// Load every unopened path diff for the exact current snapshot.
    LoadDiffs {
        commit_id: String,
        paths: Vec<String>,
    },
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
    /// Send selected text to the configured output target.
    Output { target: OutputTarget, text: String },
    /// Generate a guide through the active implementation agent.
    GenerateReviewGuide { scope: GuideScope },
    /// Save the file-pane width in terminal columns.
    SaveFilePaneWidth(u16),
    /// Save the selected text output target.
    SaveOutputTarget(OutputTarget),
    /// Stop the application.
    Quit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Selection {
    pub(super) anchor: usize,
    pub(super) cursor: usize,
    pub(super) fixed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Search {
    pub(super) query: String,
    pub(super) origin: usize,
    pub(super) origin_location: Option<ReviewLocation>,
    pub(super) editing: bool,
    pub(super) pending: Vec<SearchDirection>,
}

#[derive(Clone, Debug)]
pub(super) struct PendingGuideJump {
    pub(super) target: GuideTarget,
    pub(super) origin: Option<ReviewLocation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PendingReview {
    pub(super) path: String,
    pub(super) previous_status: ReviewStatus,
    pub(super) optimistic_status: ReviewStatus,
    pub(super) next_path: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct LocationList {
    pub(super) operation: Operation,
    pub(super) locations: Vec<SourceLocation>,
    pub(super) selected: usize,
    pub(super) scroll: usize,
    pub(super) origin_file: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ContextMenu {
    pub(super) column: u16,
    pub(super) row: u16,
    pub(super) selected: usize,
    pub(super) enabled: bool,
}

impl ContextMenu {
    pub(super) fn area(&self, bounds: Rect) -> Rect {
        let width = 22.min(bounds.width);
        let height = 5.min(bounds.height);
        Rect::new(
            self.column.min(bounds.right().saturating_sub(width)),
            self.row.min(bounds.bottom().saturating_sub(height)),
            width,
            height,
        )
    }
}

impl Selection {
    pub(super) fn range(self) -> RangeInclusive<usize> {
        self.anchor.min(self.cursor)..=self.anchor.max(self.cursor)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum DragState {
    #[default]
    None,
    Resize {
        moved: bool,
    },
    Select {
        anchor: usize,
        moved: bool,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DiffControl {
    ExpandAll,
    ContractAll,
    ShowFile,
    CloseFile,
}

impl DiffControl {
    pub(super) fn title(file: Option<&ReviewFile>) -> &'static str {
        match file {
            Some(file) if file.diff.is_file_view() => FILE_CONTROL_TITLE,
            Some(file) if file.diff.can_show_file() => DIFF_CONTROLS_TITLE,
            _ => BASIC_DIFF_CONTROLS_TITLE,
        }
    }

    pub(super) fn visible(width: u16, file: Option<&ReviewFile>) -> bool {
        if file.is_some_and(|file| file.temporary) {
            return false;
        }
        let minimum = if file.is_some_and(|file| file.diff.is_file_view()) {
            u16::try_from(FILE_CONTROL_TITLE.width() + 2).unwrap_or(u16::MAX)
        } else {
            MIN_DIFF_CONTROLS_WIDTH
        };
        width >= minimum
    }

    pub(super) fn at(width: u16, column: u16, file: Option<&ReviewFile>) -> Option<Self> {
        let title = Self::title(file);
        let title_width = u16::try_from(title.width()).ok()?;
        let left = width.saturating_sub(title_width.saturating_add(1));
        let offset = column.checked_sub(left)?;
        if file.is_some_and(|file| file.diff.is_file_view()) {
            return (offset < title_width).then_some(Self::CloseFile);
        }
        let expand_start = 0;
        let contract_start = expand_start + u16::try_from(EXPAND_ALL_LABEL.width()).ok()? + 1;
        let show_file_start = contract_start + u16::try_from(CONTRACT_ALL_LABEL.width()).ok()? + 1;
        if offset >= expand_start && offset < contract_start - 1 {
            Some(Self::ExpandAll)
        } else if offset >= contract_start
            && offset < contract_start + u16::try_from(CONTRACT_ALL_LABEL.width()).ok()?
        {
            Some(Self::ContractAll)
        } else if file.is_some_and(|file| file.diff.can_show_file())
            && offset >= show_file_start
            && offset < title_width
        {
            Some(Self::ShowFile)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PaneLayout {
    pub(super) width: u16,
    pub(super) height: u16,
    pub(super) footer_height: u16,
    pub(super) file_width: u16,
}

impl PaneLayout {
    pub(super) fn new(width: u16, height: u16, file_width: Option<u16>) -> Self {
        let file_width = if width >= NARROW_WIDTH {
            file_width
                .unwrap_or(width * 30 / 100)
                .clamp(MIN_PANE_WIDTH, width - MIN_PANE_WIDTH)
        } else {
            width
        };
        Self {
            width,
            height,
            footer_height: 2,
            file_width,
        }
    }

    pub(super) fn is_wide(self) -> bool {
        self.width >= NARROW_WIDTH
    }

    pub(super) fn body_height(self) -> u16 {
        self.height.saturating_sub(1 + self.footer_height)
    }

    pub(super) fn contains_body(self, column: u16, row: u16) -> bool {
        column < self.width && row > 0 && row < self.height.saturating_sub(self.footer_height)
    }

    pub(super) fn contains_pane_content(self, row: u16) -> bool {
        row >= 2 && row < self.height.saturating_sub(self.footer_height + 1)
    }

    pub(super) fn focus_at(self, current: Focus, column: u16, row: u16) -> Option<Focus> {
        self.contains_body(column, row).then(|| {
            if !self.is_wide() {
                current
            } else if column < self.file_width {
                Focus::Files
            } else {
                Focus::Diff
            }
        })
    }

    pub(super) fn is_separator(self, column: u16, row: u16) -> bool {
        self.is_wide() && self.contains_body(column, row) && column.abs_diff(self.file_width) <= 1
    }

    pub(super) fn diff_control_at(
        self,
        current: Focus,
        column: u16,
        row: u16,
        file: Option<&ReviewFile>,
    ) -> Option<DiffControl> {
        let diff_width = if self.is_wide() {
            self.width.saturating_sub(self.file_width)
        } else {
            self.width
        };
        (row == 1
            && DiffControl::visible(diff_width, file)
            && self.focus_at(current, column, row) == Some(Focus::Diff))
        .then(|| DiffControl::at(self.width, column, file))
        .flatten()
    }

    pub(super) fn page_rows(self) -> usize {
        usize::from(self.body_height().saturating_sub(2).max(1))
    }

    pub(super) fn diff_content_width(self) -> u16 {
        let pane_width = if self.is_wide() {
            self.width.saturating_sub(self.file_width)
        } else {
            self.width
        };
        pane_width.saturating_sub(2).max(1)
    }

    pub(super) fn diff_content_start_column(self) -> u16 {
        if self.is_wide() {
            self.file_width.saturating_add(1)
        } else {
            1
        }
    }
}

pub(super) fn display_column_to_byte(line: &str, display_column: usize) -> usize {
    let mut display: usize = 0;
    for (byte, grapheme) in line.grapheme_indices(true) {
        let width = text_display_width(grapheme);
        if display.saturating_add(width) > display_column {
            return byte;
        }
        display += width;
    }
    line.len()
}

pub(super) fn text_display_width(text: &str) -> usize {
    if text == "\t" {
        TAB_DISPLAY_WIDTH
    } else {
        text.width()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct GuideCounter {
    pub(super) number: usize,
    pub(super) total: usize,
}

/// The complete pure review UI state.
#[derive(Debug)]
pub struct ReviewApp {
    pub(super) repository_root: PathBuf,
    pub(super) change_id: String,
    pub(super) commit_id: String,
    pub(super) description: String,
    pub(super) show_commit_message: bool,
    pub(super) files: Vec<ReviewFile>,
    pub(super) file_tree: FileTree,
    pub(super) collapsed_directories: HashSet<String>,
    pub(super) selected_file: usize,
    pub(super) file_scroll: usize,
    pub(super) file_width: Option<u16>,
    pub(super) output_target: OutputTarget,
    pub(super) drag: DragState,
    pub(super) focus: Focus,
    pub(super) selection: Option<Selection>,
    pub(super) search: Option<Search>,
    pub(super) review_in_flight: Option<PendingReview>,
    pub(super) locations: Option<LocationList>,
    pub(super) location_history: LocationHistory,
    pub(super) preview: Option<ReviewFile>,
    pub(super) hover: Option<String>,
    pub(super) hover_scroll: u16,
    pub(super) context_menu: Option<ContextMenu>,
    pub(super) toasts: ToastState,
    pub(super) lsp_initialization_toast: Option<ToastId>,
    pub(super) awaiting_g_command: bool,
    pub(super) awaiting_review_command: bool,
    pub(super) pending_guide_navigation_prefix: Option<SearchDirection>,
    pub(super) guide_spinner_frame: Option<usize>,
    pub(super) guide_items: Vec<GuideItem>,
    pub(super) guide_item_counters: Vec<Option<GuideCounter>>,
    pub(super) pending_guide_jump: Option<PendingGuideJump>,
    pub(super) highlighter: SyntaxHighlighter,
    pub(super) palette: Palette,
    pub(super) width: u16,
    pub(super) height: u16,
}

impl Default for ReviewApp {
    fn default() -> Self {
        Self::new(
            Theme::default(),
            None,
            OutputTarget::default(),
            PathBuf::new(),
        )
    }
}

impl ReviewApp {
    fn finish_lsp_initialization(&mut self) {
        if let Some(id) = self.lsp_initialization_toast.take() {
            self.toasts.finish_toast(id);
        }
    }

    /// Create an empty review UI with stored settings.
    pub fn new(
        theme: Theme,
        file_width: Option<u16>,
        output_target: OutputTarget,
        repository_root: PathBuf,
    ) -> Self {
        Self {
            repository_root,
            change_id: String::new(),
            commit_id: String::new(),
            description: String::new(),
            show_commit_message: false,
            files: Vec::new(),
            file_tree: FileTree::default(),
            collapsed_directories: HashSet::new(),
            selected_file: 0,
            file_scroll: 0,
            file_width,
            output_target,
            drag: DragState::None,
            focus: Focus::Files,
            selection: None,
            search: None,
            review_in_flight: None,
            locations: None,
            location_history: LocationHistory::default(),
            preview: None,
            hover: None,
            hover_scroll: 0,
            context_menu: None,
            toasts: ToastState::default(),
            lsp_initialization_toast: None,
            awaiting_g_command: false,
            awaiting_review_command: false,
            pending_guide_navigation_prefix: None,
            guide_spinner_frame: None,
            guide_items: Vec::new(),
            guide_item_counters: Vec::new(),
            pending_guide_jump: None,
            highlighter: SyntaxHighlighter::new(theme),
            palette: theme.palette,
            width: 80,
            height: 24,
        }
    }

    pub(super) fn location_file_index(&self, location: &SourceLocation) -> Option<usize> {
        let path = location.review_path(&self.repository_root)?;
        self.files.iter().position(|file| file.path == path)
    }

    /// Apply one input and return any work for the I/O layer.
    pub fn update(&mut self, message: Message) -> Action {
        match message {
            message @ (Message::FilesLoaded { .. }
            | Message::DiffLoaded { .. }
            | Message::DiffFailed { .. }
            | Message::ReviewFinished { .. }
            | Message::OutputFinished { .. }) => self.update_repository(message),
            message @ (Message::ReviewGuideStatus { .. } | Message::ReviewGuideLoaded { .. }) => {
                self.update_review_guide(message)
            }
            message @ (Message::Lsp(_)
            | Message::SourceFailed { .. }
            | Message::SourceLoaded { .. }) => self.update_source(message),
            message @ (Message::Tick(_) | Message::Resize { .. }) => self.update_view(&message),
            message @ (Message::MouseScroll { .. }
            | Message::MouseClick { .. }
            | Message::MouseControlClick { .. }
            | Message::MouseDoubleClick { .. }
            | Message::MouseRightClick { .. }
            | Message::MouseDrag { .. }
            | Message::MouseRelease) => self.update_mouse(&message),
            Message::Key(key) => self.key(key),
        }
    }

    fn update_repository(&mut self, message: Message) -> Action {
        match message {
            Message::FilesLoaded {
                change_id,
                commit_id,
                description,
                files,
            } => self.load_files(change_id, commit_id, description, files),
            Message::DiffLoaded {
                commit_id,
                path,
                rows,
                old_content,
                new_content,
            } => self.load_diff(
                &commit_id,
                &path,
                rows,
                old_content.as_deref(),
                new_content.as_deref(),
            ),
            Message::DiffFailed {
                commit_id, path, ..
            } => {
                self.fail_diff(&commit_id, &path);
                Action::None
            }
            Message::ReviewFinished {
                change_id,
                path,
                result,
            } => self.finish_review(&change_id, &path, result),
            Message::OutputFinished { delivered } => {
                if delivered {
                    self.selection = None;
                }
                Action::None
            }
            _ => unreachable!("repository message group accepts only repository messages"),
        }
    }

    fn update_review_guide(&mut self, message: Message) -> Action {
        match message {
            Message::ReviewGuideStatus {
                review_checkpoint,
                generating,
                message,
            } => self.update_guide_status(
                &review_checkpoint.review_unit,
                &review_checkpoint.checkpoint,
                generating,
                message,
            ),
            Message::ReviewGuideLoaded {
                review_checkpoint,
                items,
            } => self.load_guide(
                &review_checkpoint.review_unit,
                &review_checkpoint.checkpoint,
                items,
            ),
            _ => unreachable!("guide message group accepts only guide messages"),
        }
    }

    fn update_source(&mut self, message: Message) -> Action {
        match message {
            Message::Lsp(event) => self.update_from_lsp_event(event),
            Message::SourceFailed {
                snapshot_id,
                message,
            } => {
                if snapshot_id == self.commit_id {
                    self.toasts.push(message, ToastKind::Error);
                }
                Action::None
            }
            Message::SourceLoaded {
                snapshot_id,
                location,
                content,
                mode,
            } => self.load_source(&snapshot_id, &location, &content, mode),
            _ => unreachable!("source message group accepts only source messages"),
        }
    }

    fn update_view(&mut self, message: &Message) -> Action {
        match message {
            Message::Tick(now) => {
                self.toasts.expire(*now);
                if let Some(frame) = &mut self.guide_spinner_frame {
                    *frame = frame.saturating_add(1);
                }
            }
            Message::Resize { width, height } => {
                if (self.width, self.height) != (*width, *height) {
                    self.width = *width;
                    self.height = *height;
                    self.keep_visible();
                }
            }
            _ => unreachable!("view message group accepts only view messages"),
        }
        Action::None
    }

    fn update_mouse(&mut self, message: &Message) -> Action {
        match message {
            Message::MouseScroll { column, row, delta } => self.mouse_scroll(*column, *row, *delta),
            Message::MouseClick {
                column,
                row,
                insert_path,
            } => self.mouse_click(*column, *row, *insert_path),
            Message::MouseControlClick { column, row } => self.mouse_control_click(*column, *row),
            Message::MouseDoubleClick { column, row } => self.mouse_double_click(*column, *row),
            Message::MouseRightClick { column, row } => self.mouse_right_click(*column, *row),
            Message::MouseDrag { column, row } => self.mouse_drag(*column, *row),
            Message::MouseRelease => self.mouse_release(),
            _ => unreachable!("mouse message group accepts only mouse messages"),
        }
    }

    fn update_guide_status(
        &mut self,
        review_unit: &str,
        checkpoint: &str,
        generating: bool,
        message: Option<String>,
    ) -> Action {
        if review_unit == self.change_id && checkpoint == self.commit_id {
            self.guide_spinner_frame = generating.then_some(0);
            if let Some(message) = message {
                self.toasts.push(message, ToastKind::Error);
            }
        }
        Action::None
    }

    fn load_guide(&mut self, review_unit: &str, checkpoint: &str, items: Vec<GuideItem>) -> Action {
        if review_unit == self.change_id && checkpoint == self.commit_id {
            self.guide_items = items;
            self.rebuild_guide_item_counters();
            self.guide_spinner_frame = None;
        }
        Action::None
    }

    fn update_from_lsp_event(&mut self, event: Event) -> Action {
        match event {
            Event::Initializing => {
                self.finish_lsp_initialization();
                self.lsp_initialization_toast =
                    Some(self.toasts.start_long_toast("Starting rust-analyzer..."));
                Action::None
            }
            Event::Ready => {
                self.finish_lsp_initialization();
                self.toasts.push("rust-analyzer ready", ToastKind::Info);
                Action::None
            }
            Event::Failed {
                toast_id,
                snapshot_id,
                message,
            } => {
                self.finish_lsp_initialization();
                if let Some(toast_id) = toast_id {
                    self.toasts.finish_toast(toast_id);
                }
                if snapshot_id.is_none_or(|snapshot_id| snapshot_id == self.commit_id) {
                    self.toasts.push(message, ToastKind::Error);
                }
                Action::None
            }
            Event::Hover {
                toast_id,
                snapshot_id,
                markdown,
            } => {
                self.toasts.finish_toast(toast_id);
                if snapshot_id == self.commit_id {
                    self.hover = markdown;
                    self.hover_scroll = 0;
                    if self.hover.is_none() {
                        self.toasts.push("No documentation found", ToastKind::Info);
                    }
                }
                Action::None
            }
            Event::Locations {
                toast_id,
                operation,
                snapshot_id,
                locations,
            } => {
                self.toasts.finish_toast(toast_id);
                self.load_locations(operation, &snapshot_id, locations)
            }
        }
    }

    /// Create a side-effect-free Ratatui view.
    pub fn view(&self) -> ReviewView<'_> {
        ReviewView(self)
    }

    fn load_files(
        &mut self,
        change_id: String,
        commit_id: String,
        description: String,
        mut files: Vec<ReviewFile>,
    ) -> Action {
        self.set_file_disk_paths(&mut files);
        let same_change = self.change_id == change_id;
        let same_snapshot = self.commit_id == commit_id;
        if !same_snapshot {
            self.clear_checkpoint_guide_state();
        }
        let refreshed_cursor = (same_change && !same_snapshot)
            .then(|| self.selected().and_then(ReviewFile::cursor_location))
            .flatten();
        let selected_path = same_change
            .then(|| self.selected().map(|file| file.path.clone()))
            .flatten();
        let expand = if same_change {
            self.refresh_files(&mut files, same_snapshot)
        } else {
            self.reset_for_new_change();
            Vec::new()
        };
        for path in expand {
            self.expand_file_parents(&path);
        }
        self.change_id = change_id;
        self.commit_id = commit_id;
        self.description = description;
        self.files = files;
        self.rebuild_guide_item_counters();
        self.rebuild_file_tree();
        let selected_file = selected_path
            .as_deref()
            .and_then(|path| self.files.iter().position(|file| file.path == path));
        self.selected_file = selected_file
            .unwrap_or(0)
            .min(self.files.len().saturating_sub(1));
        self.ensure_selected_file_visible();
        if selected_file.is_none() {
            self.selection = None;
        }
        self.keep_file_visible();
        match refreshed_cursor {
            Some(location) => self.restore_refreshed_cursor(location),
            None => self.load_selected_action(),
        }
    }

    fn set_file_disk_paths(&self, files: &mut [ReviewFile]) {
        for file in files {
            file.disk_path = Some(self.repository_root.join(&file.path));
        }
    }

    fn clear_checkpoint_guide_state(&mut self) {
        self.guide_items.clear();
        self.guide_item_counters.clear();
        self.guide_spinner_frame = None;
        self.pending_guide_jump = None;
    }

    fn refresh_files(&mut self, files: &mut Vec<ReviewFile>, same_snapshot: bool) -> Vec<String> {
        let mut expand = Vec::new();
        for file in &mut *files {
            self.refresh_file(file, same_snapshot, &mut expand);
        }
        if same_snapshot {
            files.extend(self.files.iter().filter(|file| file.temporary).cloned());
        } else {
            self.clear_transient_views();
        }
        expand
    }

    fn refresh_file(&self, file: &mut ReviewFile, same_snapshot: bool, expand: &mut Vec<String>) {
        if let Some(pending) = self
            .review_in_flight
            .as_ref()
            .filter(|pending| pending.path == file.path)
        {
            file.status = pending.optimistic_status;
        }
        let Some(old) = self.files.iter().find(|old| old.path == file.path) else {
            return;
        };
        if needs_parent_expansion(file.status, old.status) {
            expand.push(file.path.clone());
        }
        file.cursor = old.cursor;
        file.scroll = old.scroll;
        file.column = old.column;
        if same_snapshot {
            file.source_location.clone_from(&old.source_location);
            file.diff.clone_from(&old.diff);
            file.loading = old.loading;
        }
    }

    fn clear_transient_views(&mut self) {
        self.locations = None;
        self.preview = None;
        self.hover = None;
        self.context_menu = None;
    }

    fn reset_for_new_change(&mut self) {
        self.selection = None;
        self.search = None;
        self.show_commit_message = false;
        self.file_scroll = 0;
        self.focus = Focus::Files;
        self.review_in_flight = None;
        self.clear_transient_views();
        self.collapsed_directories.clear();
    }

    fn restore_refreshed_cursor(&mut self, location: SourceLocation) -> Action {
        let Some(index) = self.location_file_index(&location) else {
            return self.load_external_location(location);
        };
        self.selected_file = index;
        self.focus = Focus::Diff;
        let path = self.files[index].path.clone();
        self.files[index].disk_path = Some(location.path.clone());
        let page = self.page_rows();
        let _ = self.files[index].reveal_location(&location, page);
        self.expand_file_parents(&path);
        self.rebuild_file_tree();
        self.keep_visible();
        self.load_selected_action()
    }

    fn load_diff(
        &mut self,
        commit_id: &str,
        path: &str,
        rows: Vec<DiffRow>,
        old_content: Option<&[u8]>,
        new_content: Option<&[u8]>,
    ) -> Action {
        if self.commit_id != commit_id {
            return Action::None;
        }
        let rows = self
            .highlighter
            .highlight(path, rows, old_content, new_content);
        let Some(file) = self.files.iter_mut().find(|file| file.path == path) else {
            return Action::None;
        };
        let cursor = file.cursor_location();
        file.diff = DiffPresentation::new(rows);
        file.loading = false;
        file.cursor = file.cursor.min(file.diff.len().saturating_sub(1));
        file.scroll = file.scroll.min(file.cursor);
        self.rebuild_guide_item_counters();
        if self
            .selected()
            .is_some_and(|selected| selected.path == path)
        {
            self.selection = None;
            if self.search.as_ref().is_some_and(|search| search.editing) {
                self.update_search_match();
            } else {
                self.keep_visible();
            }
        }
        if self
            .selected()
            .is_some_and(|selected| selected.path == path)
            && let Some(location) = cursor
        {
            self.reveal_location(&location);
        }
        self.finish_pending_search();
        self.finish_pending_guide_jump(path);
        let preview_location = self.locations.as_ref().and_then(|list| {
            list.locations
                .get(list.selected)
                .filter(|location| {
                    location.review_path(&self.repository_root).as_deref() == Some(path)
                })
                .cloned()
        });
        if let Some(location) = preview_location {
            let _ = self.preview_location(location);
        }
        Action::None
    }

    fn fail_diff(&mut self, commit_id: &str, path: &str) {
        if self.commit_id != commit_id {
            return;
        }
        if let Some(file) = self.files.iter_mut().find(|file| file.path == path) {
            file.loading = false;
        }
        if self
            .pending_guide_jump
            .as_ref()
            .is_some_and(|pending| pending.target.path() == path)
        {
            self.pending_guide_jump = None;
        }
        self.finish_pending_search();
    }

    fn finish_review(
        &mut self,
        change_id: &str,
        path: &str,
        result: Result<ReviewState, ()>,
    ) -> Action {
        if self.change_id != change_id {
            return Action::None;
        }
        let pending = match self.review_in_flight.as_ref() {
            Some(pending) if pending.path == path => self.review_in_flight.take(),
            Some(_) => return Action::None,
            None => None,
        };
        let action = if let Ok(state) = result {
            self.finish_successful_review(path, state, pending.as_ref())
        } else {
            self.restore_failed_review(path, pending.as_ref());
            Action::None
        };
        self.rebuild_guide_item_counters();
        action
    }

    fn finish_successful_review(
        &mut self,
        path: &str,
        state: ReviewState,
        pending: Option<&PendingReview>,
    ) -> Action {
        let mut expand = false;
        if let Some(file) = self.files.iter_mut().find(|file| file.path == path) {
            expand = needs_parent_expansion(state.status, file.status);
            file.status = state.status;
            if state.status == ReviewStatus::Unreviewed {
                file.diff = DiffPresentation::default();
            }
        }
        if expand && self.expand_file_parents(path) {
            self.rebuild_file_tree();
        }
        let selected_pending_next = pending.is_some_and(|pending| {
            pending.optimistic_status == ReviewStatus::Reviewed
                && state.status == ReviewStatus::Reviewed
                && self.selected_is_pending_next(pending)
        });
        let selected_review_file = state.status.needs_review()
            && self
                .selected()
                .is_some_and(|selected| selected.path == path);
        if selected_pending_next || selected_review_file {
            return self.load_selected_action();
        }
        Action::None
    }

    fn restore_failed_review(&mut self, path: &str, pending: Option<&PendingReview>) {
        let Some(pending) = pending else {
            return;
        };
        if let Some(file) = self.files.iter_mut().find(|file| file.path == path) {
            file.status = pending.previous_status;
        }
    }

    fn selected_is_pending_next(&self, pending: &PendingReview) -> bool {
        self.selected()
            .is_some_and(|selected| Some(selected.path.as_str()) == pending.next_path.as_deref())
    }

    pub(super) fn remove_temporary_files(&mut self) {
        self.files.retain(|file| !file.temporary);
        self.rebuild_guide_item_counters();
        self.selected_file = self.selected_file.min(self.files.len().saturating_sub(1));
        self.rebuild_file_tree();
    }

    pub(super) fn rebuild_guide_item_counters(&mut self) {
        let mut ordered_items = Vec::new();
        for file in &self.files {
            if !file.status.needs_review() {
                continue;
            }
            let file_start = ordered_items.len();
            ordered_items.extend(self.guide_items.iter().enumerate().filter_map(
                |(item_index, item)| {
                    if item.target.path() != file.path {
                        return None;
                    }
                    match DiffView::guide_target_position(file, &item.target)? {
                        GuideTargetPosition::Unloaded => Some((item_index, item_index)),
                        GuideTargetPosition::Rows { first } => Some((item_index, first)),
                    }
                },
            ));
            ordered_items[file_start..].sort_by_key(|(_, row)| *row);
        }

        let total = ordered_items.len();
        let mut counters = vec![None; self.guide_items.len()];
        for (offset, (item_index, _)) in ordered_items.into_iter().enumerate() {
            counters[item_index] = Some(GuideCounter {
                number: offset.saturating_add(1),
                total,
            });
        }
        self.guide_item_counters = counters;
    }

    pub(super) fn layout(&self) -> PaneLayout {
        PaneLayout::new(self.width, self.height, self.file_width)
    }

    pub(super) fn hovered_focus(&self, column: u16, row: u16) -> Option<Focus> {
        self.layout().focus_at(self.focus, column, row)
    }

    pub(super) fn load_selected_action(&mut self) -> Action {
        let commit_id = self.commit_id.clone();
        let Some(file) = self.files.get_mut(self.selected_file) else {
            return Action::None;
        };
        file.start_diff_load()
            .map_or(Action::None, |path| Action::LoadDiff { commit_id, path })
    }

    pub(super) fn load_all_diffs_action(&mut self) -> Action {
        let paths = self
            .files
            .iter_mut()
            .filter_map(ReviewFile::start_diff_load)
            .collect::<Vec<_>>();
        if paths.is_empty() {
            Action::None
        } else {
            Action::LoadDiffs {
                commit_id: self.commit_id.clone(),
                paths,
            }
        }
    }

    pub(super) fn keep_visible(&mut self) {
        self.keep_file_visible();
        let layout = self.layout();
        let page = layout.page_rows();
        let scroll = self.displayed().map(|file| {
            DiffView(self)
                .viewport(file, layout.diff_content_width(), false)
                .scroll_with_cursor_visible(file, page)
        });
        let Some(file) = self.displayed_mut() else {
            return;
        };
        file.scroll = scroll.unwrap_or(0);
    }

    pub(super) fn keep_file_visible(&mut self) {
        let page = self.page_rows();
        let Some(row) = self.file_tree.row_for_file(self.selected_file) else {
            self.file_scroll = 0;
            return;
        };
        if row < self.file_scroll {
            self.file_scroll = row;
        } else if row >= self.file_scroll + page {
            self.file_scroll = row + 1 - page;
        }
    }

    pub(super) fn page_rows(&self) -> usize {
        self.layout().page_rows()
    }

    pub(super) fn half_page_rows(&self) -> isize {
        isize::try_from(self.page_rows().div_ceil(2)).unwrap_or(isize::MAX)
    }

    pub(super) fn focus_len(&self) -> usize {
        match self.focus {
            Focus::Files => self.file_tree.visible_file_count(),
            Focus::Diff => self.selected().map_or(0, |file| file.diff.len()),
        }
    }

    pub(super) fn selected(&self) -> Option<&ReviewFile> {
        self.files.get(self.selected_file)
    }

    pub(super) fn select_file(&mut self, file: usize) {
        if file != self.selected_file {
            if let Some(selected) = self.files.get_mut(self.selected_file) {
                selected.clear_source_location();
            }
            self.selected_file = file;
        }
    }

    pub(super) fn displayed(&self) -> Option<&ReviewFile> {
        self.preview.as_ref().or_else(|| self.selected())
    }

    pub(super) fn displayed_mut(&mut self) -> Option<&mut ReviewFile> {
        if self.preview.is_some() {
            self.preview.as_mut()
        } else {
            self.files.get_mut(self.selected_file)
        }
    }

    pub(super) fn file_matches_search(&self, index: usize) -> bool {
        let Some(search) = self
            .search
            .as_ref()
            .filter(|search| !search.query.is_empty())
        else {
            return false;
        };
        // ponytail: cache matches if rescanning loaded diffs makes large reviews stutter.
        self.files
            .get(index)
            .is_some_and(|file| file.diff.matching_rows(&search.query).next().is_some())
    }

    pub(super) fn rebuild_file_tree(&mut self) {
        self.file_tree = FileTree::new(
            self.files
                .iter()
                .map(|file| (file.path.as_str(), file.display_path.as_str())),
            &self.collapsed_directories,
        );
    }

    pub(super) fn expand_file_parents(&mut self, path: &str) -> bool {
        path.match_indices('/')
            .map(|(index, _)| &path[..index])
            .fold(false, |changed, parent| {
                self.collapsed_directories.remove(parent) || changed
            })
    }

    pub(super) fn ensure_selected_file_visible(&mut self) {
        if self.file_tree.row_for_file(self.selected_file).is_none()
            && let Some(file) = self.file_tree.nearest_visible_file(self.selected_file)
        {
            self.select_file(file);
            self.selection = None;
        }
    }

    pub(super) fn commit_title(&self) -> &str {
        self.description
            .lines()
            .next()
            .filter(|title| !title.is_empty())
            .unwrap_or("(no description set)")
    }

    pub(super) fn commit_title_at(&self, column: u16, row: u16) -> bool {
        row == 0 && column > 0 && usize::from(column) <= self.commit_title().width()
    }
}
