//! Pure review state and navigation.

use std::collections::HashSet;
use std::ops::RangeInclusive;

use pr_core::diff::DiffRow;
use pr_core::herdr::InsertResult;
use pr_core::repository::{ChangeKind, ChangedFile};
use unicode_width::UnicodeWidthStr;

use crate::file_tree::FileTree;
use crate::highlight::DiffHighlighter;
use crate::presentation::{DiffPresentation, SearchDirection};
use crate::review::{ReviewState, ReviewStatus};
use crate::theme::{Palette, Theme};

const NARROW_WIDTH: u16 = 72;
const MIN_PANE_WIDTH: u16 = 16;
const DIFF_CONTROLS_TITLE: &str = "[←→] [→←]";
const EXPAND_ALL_LABEL: &str = "[←→]";
const CONTRACT_ALL_LABEL: &str = "[→←]";
const MIN_DIFF_CONTROLS_WIDTH: u16 = 32;

mod views;

pub use views::ReviewView;

/// The pane that receives navigation keys.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Focus {
    /// The changed-file list.
    Files,
    /// The selected file diff.
    Diff,
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
    /// Escaped text shown to the user.
    display_path: String,
    /// Current review state.
    pub status: ReviewStatus,
    change: ChangeKind,
    lines_added: u64,
    lines_removed: u64,
    diff: DiffPresentation,
    cursor: usize,
    scroll: usize,
    loading: bool,
}

impl ReviewFile {
    /// Create one file summary before its diff arrives.
    pub fn new(path: impl Into<String>, status: ReviewStatus) -> Self {
        let path = path.into();
        Self {
            display_path: path.clone(),
            path,
            status,
            change: ChangeKind::Modified,
            lines_added: 0,
            lines_removed: 0,
            diff: DiffPresentation::default(),
            cursor: 0,
            scroll: 0,
            loading: false,
        }
    }

    pub(crate) fn from_changed(file: &ChangedFile, status: ReviewStatus) -> Self {
        Self {
            display_path: file.display_path.clone(),
            change: file.change,
            lines_added: file.lines_added,
            lines_removed: file.lines_removed,
            ..Self::new(file.review_path().display(), status)
        }
    }

    fn has_notice(&self) -> bool {
        self.diff.has_notice()
    }

    fn marker(&self) -> &'static str {
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

    fn scroll_by(&mut self, delta: isize, page: usize) {
        let last = self.diff.len().saturating_sub(page);
        self.scroll = self.scroll.saturating_add_signed(delta).min(last);
    }

    fn start_diff_load(&mut self) -> Option<String> {
        if !self.diff.is_empty() || self.loading {
            return None;
        }
        self.loading = true;
        Some(self.path.clone())
    }
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
    /// An excerpt insertion result.
    InsertFinished(Result<InsertResult, ()>),
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
    /// Set the selected path review state.
    SetReviewed { path: String, reviewed: bool },
    /// Insert one valid unified-diff excerpt.
    Insert { text: String },
    /// Save the file-pane width in terminal columns.
    SaveFilePaneWidth(u16),
    /// Stop the application.
    Quit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Selection {
    anchor: usize,
    cursor: usize,
    fixed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Search {
    query: String,
    origin: usize,
    editing: bool,
    pending: Vec<SearchDirection>,
}

impl Selection {
    fn range(self) -> RangeInclusive<usize> {
        self.anchor.min(self.cursor)..=self.anchor.max(self.cursor)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum DragState {
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
enum DiffControl {
    ExpandAll,
    ContractAll,
}

impl DiffControl {
    fn at(width: u16, column: u16) -> Option<Self> {
        let title_width = u16::try_from(DIFF_CONTROLS_TITLE.width()).ok()?;
        let left = width.saturating_sub(title_width.saturating_add(1));
        let offset = column.checked_sub(left)?;
        let expand_start = 0;
        let contract_start = expand_start + u16::try_from(EXPAND_ALL_LABEL.width()).ok()? + 1;
        if offset >= expand_start && offset < contract_start - 1 {
            Some(Self::ExpandAll)
        } else if offset >= contract_start
            && offset < contract_start + u16::try_from(CONTRACT_ALL_LABEL.width()).ok()?
        {
            Some(Self::ContractAll)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PaneLayout {
    width: u16,
    height: u16,
    footer_height: u16,
    file_width: u16,
}

impl PaneLayout {
    fn new(width: u16, height: u16, file_width: Option<u16>) -> Self {
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
            footer_height: 1,
            file_width,
        }
    }

    fn is_wide(self) -> bool {
        self.width >= NARROW_WIDTH
    }

    fn body_height(self) -> u16 {
        self.height.saturating_sub(1 + self.footer_height)
    }

    fn contains_body(self, column: u16, row: u16) -> bool {
        column < self.width && row > 0 && row < self.height.saturating_sub(self.footer_height)
    }

    fn contains_pane_content(self, row: u16) -> bool {
        row >= 2 && row < self.height.saturating_sub(self.footer_height + 1)
    }

    fn focus_at(self, current: Focus, column: u16, row: u16) -> Option<Focus> {
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

    fn is_separator(self, column: u16, row: u16) -> bool {
        self.is_wide() && self.contains_body(column, row) && column.abs_diff(self.file_width) <= 1
    }

    fn diff_control_at(self, current: Focus, column: u16, row: u16) -> Option<DiffControl> {
        let diff_width = if self.is_wide() {
            self.width.saturating_sub(self.file_width)
        } else {
            self.width
        };
        (row == 1
            && diff_width >= MIN_DIFF_CONTROLS_WIDTH
            && self.focus_at(current, column, row) == Some(Focus::Diff))
        .then(|| DiffControl::at(self.width, column))
        .flatten()
    }

    fn page_rows(self) -> usize {
        usize::from(self.body_height().saturating_sub(2).max(1))
    }
}

/// The complete pure review UI state.
#[derive(Debug)]
pub struct ReviewApp {
    change_id: String,
    commit_id: String,
    description: String,
    show_commit_message: bool,
    files: Vec<ReviewFile>,
    file_tree: FileTree,
    collapsed_directories: HashSet<String>,
    selected_file: usize,
    file_scroll: usize,
    file_width: Option<u16>,
    drag: DragState,
    focus: Focus,
    selection: Option<Selection>,
    search: Option<Search>,
    review_in_flight: Option<bool>,
    highlighter: DiffHighlighter,
    palette: Palette,
    width: u16,
    height: u16,
}

impl Default for ReviewApp {
    fn default() -> Self {
        Self::new(Theme::default(), None)
    }
}

impl ReviewApp {
    pub(crate) fn new(theme: Theme, file_width: Option<u16>) -> Self {
        Self {
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
            drag: DragState::None,
            focus: Focus::Files,
            selection: None,
            search: None,
            review_in_flight: None,
            highlighter: DiffHighlighter::new(theme),
            palette: theme.palette,
            width: 80,
            height: 24,
        }
    }

    /// Apply one input and return any work for the I/O layer.
    pub fn update(&mut self, message: Message) -> Action {
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
            } => {
                self.load_diff(
                    &commit_id,
                    &path,
                    rows,
                    old_content.as_deref(),
                    new_content.as_deref(),
                );
                Action::None
            }
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
            Message::InsertFinished(result) => {
                if matches!(result, Ok(InsertResult::Inserted { .. })) {
                    self.selection = None;
                }
                Action::None
            }
            Message::Resize { width, height } => {
                if (self.width, self.height) != (width, height) {
                    self.width = width;
                    self.height = height;
                    self.keep_visible();
                }
                Action::None
            }
            Message::MouseScroll { column, row, delta } => self.mouse_scroll(column, row, delta),
            Message::MouseClick {
                column,
                row,
                insert_path,
            } => self.mouse_click(column, row, insert_path),
            Message::MouseDrag { column, row } => self.mouse_drag(column, row),
            Message::MouseRelease => self.mouse_release(),
            Message::Key(key) => self.key(key),
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
        let same_change = self.change_id == change_id;
        let same_snapshot = self.commit_id == commit_id;
        let selected_path = same_change
            .then(|| self.selected().map(|file| file.path.clone()))
            .flatten();
        if same_change {
            for file in &mut files {
                if let Some(old) = self.files.iter().find(|old| old.path == file.path) {
                    file.cursor = old.cursor;
                    file.scroll = old.scroll;
                    if same_snapshot {
                        file.diff.clone_from(&old.diff);
                        file.loading = old.loading;
                    }
                }
            }
        } else {
            self.selection = None;
            self.search = None;
            self.show_commit_message = false;
            self.file_scroll = 0;
            self.focus = Focus::Files;
            self.review_in_flight = None;
            self.collapsed_directories.clear();
        }

        self.change_id = change_id;
        self.commit_id = commit_id;
        self.description = description;
        self.files = files;
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
        self.load_selected_action()
    }

    fn load_diff(
        &mut self,
        commit_id: &str,
        path: &str,
        rows: Vec<DiffRow>,
        old_content: Option<&[u8]>,
        new_content: Option<&[u8]>,
    ) {
        if self.commit_id != commit_id {
            return;
        }
        let rows = self
            .highlighter
            .highlight(path, rows, old_content, new_content);
        let Some(file) = self.files.iter_mut().find(|file| file.path == path) else {
            return;
        };
        file.diff = DiffPresentation::new(rows);
        file.loading = false;
        file.cursor = file.cursor.min(file.diff.len().saturating_sub(1));
        file.scroll = file.scroll.min(file.cursor);
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
        self.finish_pending_search();
    }

    fn fail_diff(&mut self, commit_id: &str, path: &str) {
        if self.commit_id != commit_id {
            return;
        }
        if let Some(file) = self.files.iter_mut().find(|file| file.path == path) {
            file.loading = false;
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
        let marking_reviewed = self.review_in_flight == Some(true);
        self.review_in_flight = None;
        match result {
            Ok(state) => {
                if let Some(file) = self.files.iter_mut().find(|file| file.path == path) {
                    file.status = state.status;
                    if state.status == ReviewStatus::Unreviewed {
                        file.diff = DiffPresentation::default();
                    }
                }
                let next_unreviewed = self
                    .file_tree
                    .visible_files()
                    .skip_while(|file| *file != self.selected_file)
                    .skip(1)
                    .find(|file| self.files[*file].status == ReviewStatus::Unreviewed);
                if marking_reviewed
                    && state.status == ReviewStatus::Reviewed
                    && self
                        .selected()
                        .is_some_and(|selected| selected.path == path)
                    && let Some(next_file) = next_unreviewed
                {
                    self.selected_file = next_file;
                    self.selection = None;
                    self.keep_visible();
                    return self.load_selected_action();
                }
                if state.status != ReviewStatus::Reviewed
                    && self
                        .selected()
                        .is_some_and(|selected| selected.path == path)
                {
                    return self.load_selected_action();
                }
            }
            Err(_) => {}
        }
        Action::None
    }

    fn key(&mut self, key: Key) -> Action {
        if self.search.as_ref().is_some_and(|search| search.editing) {
            return self.search_key(key);
        }
        match key {
            Key::Char('/') => {
                self.focus = Focus::Diff;
                self.selection = None;
                self.search = Some(Search {
                    query: String::new(),
                    origin: self.selected().map_or(0, |file| file.cursor),
                    editing: true,
                    pending: Vec::new(),
                });
                self.load_all_diffs_action()
            }
            Key::Char('n') => {
                self.repeat_search(SearchDirection::Forward);
                Action::None
            }
            Key::Char('p') => {
                self.repeat_search(SearchDirection::Backward);
                Action::None
            }
            Key::Quit | Key::Char('q') => Action::Quit,
            Key::CommitMessage | Key::Char('c') => {
                self.show_commit_message = !self.show_commit_message;
                Action::None
            }
            Key::Tab => {
                self.focus = match self.focus {
                    Focus::Files => Focus::Diff,
                    Focus::Diff => Focus::Files,
                };
                Action::None
            }
            Key::Escape => {
                self.show_commit_message = false;
                self.selection = None;
                self.search = None;
                Action::None
            }
            Key::Enter if self.focus == Focus::Files => {
                self.selected().map_or(Action::None, |file| Action::Insert {
                    text: file.path.clone(),
                })
            }
            Key::Enter => self.insert(),
            Key::Space | Key::Char(' ') => self.toggle_review(),
            Key::Visual | Key::Char('v' | 'V') if self.focus == Focus::Diff => {
                self.visual();
                Action::None
            }
            Key::Expand | Key::Char('l') if self.focus == Focus::Diff => {
                self.expand_gap();
                Action::None
            }
            Key::Down | Key::Char('j') => self.navigate(1),
            Key::Up | Key::Char('k') => self.navigate(-1),
            Key::First | Key::Char('g') => self.navigate_to(0),
            Key::Last | Key::Char('G') => {
                let last = self.focus_len().saturating_sub(1);
                self.navigate_to(last)
            }
            Key::HalfPageDown => self.navigate(self.half_page_rows()),
            Key::HalfPageUp => self.navigate(-self.half_page_rows()),
            Key::Char(_) | Key::Backspace | Key::Visual | Key::Expand => Action::None,
        }
    }

    fn search_key(&mut self, key: Key) -> Action {
        match key {
            Key::Char(character) => {
                if let Some(search) = &mut self.search {
                    search.query.push(character);
                }
                self.update_search_match();
            }
            Key::Backspace => {
                if let Some(search) = &mut self.search {
                    search.query.pop();
                }
                self.update_search_match();
            }
            Key::Enter => {
                if let Some(search) = &mut self.search {
                    search.editing = false;
                }
            }
            Key::Escape => {
                self.search = None;
            }
            _ => {}
        }
        Action::None
    }

    fn update_search_match(&mut self) {
        let Some(search) = &self.search else {
            return;
        };
        if search.query.is_empty() {
            if let Some(file) = self.files.get_mut(self.selected_file) {
                file.cursor = search.origin.min(file.diff.len().saturating_sub(1));
            }
            self.keep_visible();
            return;
        }
        let target = self.selected().and_then(|file| {
            file.diff
                .find_matching_row(&search.query, search.origin, SearchDirection::Forward)
        });
        self.move_diff_cursor(target);
    }

    fn repeat_search(&mut self, direction: SearchDirection) {
        let Some(search) = self.search.as_ref().filter(|search| !search.editing) else {
            return;
        };
        if self.files.iter().any(|file| file.loading) {
            if let Some(search) = &mut self.search {
                search.pending.push(direction);
            }
            return;
        }
        let current = (
            self.selected_file,
            self.selected().map_or(0, |file| file.cursor),
        );
        let matches = self
            .files
            .iter()
            .enumerate()
            .flat_map(|(file, review_file)| {
                review_file
                    .diff
                    .matching_rows(&search.query)
                    .map(move |row| (file, row))
            })
            .collect::<Vec<_>>();
        let target = match direction {
            SearchDirection::Backward => matches
                .iter()
                .rev()
                .copied()
                .find(|target| *target < current)
                .or_else(|| matches.last().copied()),
            SearchDirection::Forward => matches
                .iter()
                .copied()
                .find(|target| *target > current)
                .or_else(|| matches.first().copied()),
        };
        if let Some((file, row)) = target {
            self.selected_file = file;
            self.move_diff_cursor(Some(row));
        }
    }

    fn finish_pending_search(&mut self) {
        if self.files.iter().any(|file| file.loading) {
            return;
        }
        let pending = self
            .search
            .as_mut()
            .map(|search| std::mem::take(&mut search.pending))
            .unwrap_or_default();
        for direction in pending {
            self.repeat_search(direction);
        }
    }

    fn move_diff_cursor(&mut self, target: Option<usize>) {
        let Some(target) = target else {
            return;
        };
        if let Some(file) = self.files.get_mut(self.selected_file) {
            file.cursor = target;
        }
        self.selection = None;
        self.keep_visible();
    }

    fn toggle_review(&mut self) -> Action {
        if self.review_in_flight.is_some() {
            return Action::None;
        }
        let Some(file) = self.selected() else {
            return Action::None;
        };
        let path = file.path.clone();
        let reviewed = file.status != ReviewStatus::Reviewed;
        let action = Action::SetReviewed { path, reviewed };
        self.review_in_flight = Some(reviewed);
        action
    }

    fn insert(&mut self) -> Action {
        let Some(file) = self.selected() else {
            return Action::None;
        };
        let Some(selection) = self.selection else {
            return Action::None;
        };
        let range = selection.range();
        if !range.clone().any(|index| file.diff.is_selectable(index)) {
            return Action::None;
        }
        match file.diff.excerpt(range) {
            Ok(excerpt) => Action::Insert {
                text: excerpt.into_string(),
            },
            Err(_) => Action::None,
        }
    }

    fn visual(&mut self) {
        let Some(file) = self.selected() else {
            return;
        };
        if !file.diff.is_selectable(file.cursor) {
            return;
        }
        self.selection = match self.selection {
            Some(mut selection) if !selection.fixed => {
                selection.fixed = true;
                Some(selection)
            }
            _ => Some(Selection {
                anchor: file.cursor,
                cursor: file.cursor,
                fixed: false,
            }),
        };
    }

    fn expand_gap(&mut self) {
        let Some(file) = self.files.get_mut(self.selected_file) else {
            return;
        };
        if file.diff.expand(file.cursor) {
            self.selection = None;
            self.keep_visible();
        }
    }

    fn navigate(&mut self, delta: isize) -> Action {
        let current = match self.focus {
            Focus::Files => self
                .file_tree
                .visible_file_position(self.selected_file)
                .unwrap_or(0),
            Focus::Diff => self.selected().map_or(0, |file| file.cursor),
        };
        self.navigate_to(current.saturating_add_signed(delta))
    }

    fn navigate_to(&mut self, target: usize) -> Action {
        let mut selected_changed = false;
        match self.focus {
            Focus::Files => {
                let target = target.min(self.file_tree.visible_file_count().saturating_sub(1));
                let Some(target) = self.file_tree.visible_file_at(target) else {
                    return Action::None;
                };
                if target != self.selected_file {
                    self.selected_file = target;
                    self.selection = None;
                    selected_changed = true;
                }
            }
            Focus::Diff => {
                let Some(file) = self.files.get_mut(self.selected_file) else {
                    return Action::None;
                };
                file.cursor = target.min(file.diff.len().saturating_sub(1));
                if let Some(selection) = &mut self.selection
                    && !selection.fixed
                {
                    selection.cursor = file.cursor;
                }
            }
        }
        self.keep_visible();
        if selected_changed {
            self.load_selected_action()
        } else {
            Action::None
        }
    }

    fn mouse_scroll(&mut self, column: u16, row: u16, delta: isize) -> Action {
        let Some(hovered) = self.hovered_focus(column, row) else {
            return Action::None;
        };
        if hovered == Focus::Diff {
            let page = self.page_rows();
            if let Some(file) = self.files.get_mut(self.selected_file) {
                file.scroll_by(delta, page);
            }
            return Action::None;
        }
        let focused = self.focus;
        self.focus = hovered;
        let action = self.navigate(delta);
        self.focus = focused;
        action
    }

    fn mouse_click(&mut self, column: u16, row: u16, insert_path: bool) -> Action {
        let layout = self.layout();
        if self.commit_title_at(column, row) {
            self.show_commit_message = !self.show_commit_message;
            return Action::None;
        }
        if let Some(control) = layout.diff_control_at(self.focus, column, row) {
            self.focus = Focus::Diff;
            let Some(file) = self.files.get_mut(self.selected_file) else {
                return Action::None;
            };
            let changed = match control {
                DiffControl::ExpandAll => file.diff.expand_all(),
                DiffControl::ContractAll => file.diff.contract_all(),
            };
            if changed {
                self.selection = None;
                file.cursor = file.cursor.min(file.diff.len().saturating_sub(1));
                file.scroll = file.scroll.min(file.cursor);
                self.keep_visible();
            }
            return Action::None;
        }
        self.drag = if layout.is_separator(column, row) {
            DragState::Resize { moved: false }
        } else {
            DragState::None
        };
        if matches!(self.drag, DragState::Resize { .. }) {
            return Action::None;
        }
        let Some(focus) = layout.focus_at(self.focus, column, row) else {
            return Action::None;
        };
        self.focus = focus;
        if focus == Focus::Diff && layout.contains_pane_content(row) {
            let page_row = usize::from(row - 2);
            let Some(file) = self.files.get_mut(self.selected_file) else {
                return Action::None;
            };
            file.cursor = (file.scroll + page_row).min(file.diff.len().saturating_sub(1));
            if file.diff.expand(file.cursor) {
                self.selection = None;
            } else if file.diff.is_selectable(file.cursor) {
                self.drag = DragState::Select {
                    anchor: file.cursor,
                    moved: false,
                };
            }
            self.keep_visible();
            return Action::None;
        }
        if focus != Focus::Files || !layout.contains_pane_content(row) {
            return Action::None;
        }
        let row = self.file_scroll + usize::from(row - 2);
        if let Some(directory) = self
            .file_tree
            .directory_at(row)
            .filter(|(_, depth)| {
                column == 1 + u16::try_from(depth.saturating_mul(2)).unwrap_or(u16::MAX)
            })
            .map(|(path, _)| path.to_owned())
        {
            if !self.collapsed_directories.remove(&directory) {
                self.collapsed_directories.insert(directory);
            }
            self.rebuild_file_tree();
            self.ensure_selected_file_visible();
            self.keep_file_visible();
            return self.load_selected_action();
        }
        let Some(target) = self.file_tree.file_at(row) else {
            return Action::None;
        };
        self.selected_file = target;
        self.selection = None;
        if insert_path {
            return Action::Insert {
                text: self.files[target].path.clone(),
            };
        }
        self.load_selected_action()
    }

    fn mouse_drag(&mut self, column: u16, row: u16) -> Action {
        let layout = self.layout();
        match self.drag {
            DragState::Resize { .. } if layout.is_wide() && layout.contains_body(column, row) => {
                self.file_width = Some(column.clamp(MIN_PANE_WIDTH, self.width - MIN_PANE_WIDTH));
                self.drag = DragState::Resize { moved: true };
                self.keep_visible();
            }
            DragState::Select { anchor, .. }
                if layout.focus_at(self.focus, column, row) == Some(Focus::Diff)
                    && layout.contains_pane_content(row) =>
            {
                let Some(file) = self.files.get_mut(self.selected_file) else {
                    return Action::None;
                };
                file.cursor =
                    (file.scroll + usize::from(row - 2)).min(file.diff.len().saturating_sub(1));
                self.selection = Some(Selection {
                    anchor,
                    cursor: file.cursor,
                    fixed: true,
                });
                self.drag = DragState::Select {
                    anchor,
                    moved: true,
                };
                self.keep_visible();
            }
            DragState::None | DragState::Resize { .. } | DragState::Select { .. } => {}
        }
        Action::None
    }

    fn mouse_release(&mut self) -> Action {
        let action = match self.drag {
            DragState::Resize { moved: true } => self
                .file_width
                .map_or(Action::None, Action::SaveFilePaneWidth),
            DragState::Select { moved: true, .. } => self.insert(),
            DragState::None | DragState::Resize { .. } | DragState::Select { .. } => Action::None,
        };
        self.drag = DragState::None;
        action
    }

    fn layout(&self) -> PaneLayout {
        PaneLayout::new(self.width, self.height, self.file_width)
    }

    fn hovered_focus(&self, column: u16, row: u16) -> Option<Focus> {
        self.layout().focus_at(self.focus, column, row)
    }

    fn load_selected_action(&mut self) -> Action {
        let commit_id = self.commit_id.clone();
        let Some(file) = self.files.get_mut(self.selected_file) else {
            return Action::None;
        };
        file.start_diff_load()
            .map_or(Action::None, |path| Action::LoadDiff { commit_id, path })
    }

    fn load_all_diffs_action(&mut self) -> Action {
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

    fn keep_visible(&mut self) {
        self.keep_file_visible();
        let page = self.page_rows();
        let Some(file) = self.files.get_mut(self.selected_file) else {
            return;
        };
        if file.cursor < file.scroll {
            file.scroll = file.cursor;
        } else if file.cursor >= file.scroll + page {
            file.scroll = file.cursor + 1 - page;
        }
    }

    fn keep_file_visible(&mut self) {
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

    fn page_rows(&self) -> usize {
        self.layout().page_rows()
    }

    fn half_page_rows(&self) -> isize {
        isize::try_from(self.page_rows().div_ceil(2)).unwrap_or(isize::MAX)
    }

    fn focus_len(&self) -> usize {
        match self.focus {
            Focus::Files => self.file_tree.visible_file_count(),
            Focus::Diff => self.selected().map_or(0, |file| file.diff.len()),
        }
    }

    fn selected(&self) -> Option<&ReviewFile> {
        self.files.get(self.selected_file)
    }

    fn file_matches_search(&self, index: usize) -> bool {
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

    fn rebuild_file_tree(&mut self) {
        self.file_tree = FileTree::new(
            self.files
                .iter()
                .map(|file| (file.path.as_str(), file.display_path.as_str())),
            &self.collapsed_directories,
        );
    }

    fn ensure_selected_file_visible(&mut self) {
        if self.file_tree.row_for_file(self.selected_file).is_none()
            && let Some(file) = self.file_tree.nearest_visible_file(self.selected_file)
        {
            self.selected_file = file;
            self.selection = None;
        }
    }

    fn commit_title(&self) -> &str {
        self.description
            .lines()
            .next()
            .filter(|title| !title.is_empty())
            .unwrap_or("(no description set)")
    }

    fn commit_title_at(&self, column: u16, row: u16) -> bool {
        row == 0 && column > 0 && usize::from(column) <= self.commit_title().width()
    }
}

#[cfg(test)]
mod tests {
    use pr_core::diff::DiffRow;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::{Key, Message, ReviewApp, ReviewFile};
    use crate::review::ReviewStatus;

    #[test]
    fn interactive_search_moves_and_repeats_from_the_diff_cursor() {
        let mut app = ReviewApp::default();
        app.update(Message::FilesLoaded {
            change_id: "change".to_owned(),
            commit_id: "commit".to_owned(),
            description: String::new(),
            files: vec![
                ReviewFile::new("src/lib.rs", ReviewStatus::Unreviewed),
                ReviewFile::new("src/other.rs", ReviewStatus::Reviewed),
                ReviewFile::new("src/unopened.rs", ReviewStatus::Unreviewed),
            ],
        });
        app.update(Message::DiffLoaded {
            commit_id: "commit".to_owned(),
            path: "src/lib.rs".to_owned(),
            rows: [
                "start",
                "noise",
                "filler",
                "Needle one",
                "more",
                "needle two",
            ]
            .into_iter()
            .enumerate()
            .map(|(index, text)| DiffRow::Context {
                old_line: u32::try_from(index + 1).unwrap(),
                new_line: u32::try_from(index + 1).unwrap(),
                text: format!(" {text}"),
            })
            .collect(),
            old_content: None,
            new_content: None,
        });
        app.update(Message::DiffLoaded {
            commit_id: "commit".to_owned(),
            path: "src/other.rs".to_owned(),
            rows: vec![
                DiffRow::Context {
                    old_line: 1,
                    new_line: 1,
                    text: " no match".to_owned(),
                },
                DiffRow::Context {
                    old_line: 2,
                    new_line: 2,
                    text: " needle in another file".to_owned(),
                },
            ],
            old_content: None,
            new_content: None,
        });

        assert_eq!(
            app.update(Message::Key(Key::Char('/'))),
            super::Action::LoadDiffs {
                commit_id: "commit".to_owned(),
                paths: vec!["src/unopened.rs".to_owned()],
            }
        );
        app.update(Message::Key(Key::Char('n')));
        assert_eq!(app.selected().unwrap().cursor, 1);
        app.update(Message::Key(Key::Char('e')));
        assert_eq!(app.selected().unwrap().cursor, 3);
        for character in "edle".chars() {
            app.update(Message::Key(Key::Char(character)));
        }
        assert!(app.file_matches_search(0));
        assert!(app.file_matches_search(1));
        assert!(!app.file_matches_search(2));
        let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();
        terminal
            .draw(|frame| frame.render_widget(app.view(), frame.area()))
            .unwrap();
        assert!((0..12).any(|row| {
            (0..80).any(|column| {
                let cell = &terminal.backend().buffer()[(column, row)];
                cell.fg == app.palette.deletion
                    && cell.bg == app.palette.warning
                    && cell.modifier.contains(ratatui::style::Modifier::BOLD)
            })
        }));
        app.update(Message::Key(Key::Enter));
        app.update(Message::Key(Key::Char('n')));
        app.update(Message::Key(Key::Char('n')));
        assert_eq!(app.selected().unwrap().cursor, 3);
        app.update(Message::DiffLoaded {
            commit_id: "commit".to_owned(),
            path: "src/unopened.rs".to_owned(),
            rows: vec![DiffRow::Context {
                old_line: 1,
                new_line: 1,
                text: " needle in the unopened file".to_owned(),
            }],
            old_content: None,
            new_content: None,
        });
        assert_eq!((app.selected_file, app.selected().unwrap().cursor), (1, 1));
        terminal
            .draw(|frame| frame.render_widget(app.view(), frame.area()))
            .unwrap();
        assert!((0..12).any(|row| {
            (0..80).any(|column| {
                let cell = &terminal.backend().buffer()[(column, row)];
                cell.fg == app.palette.deletion
                    && cell.bg == app.palette.warning
                    && cell.modifier.contains(ratatui::style::Modifier::BOLD)
            })
        }));
        app.update(Message::Key(Key::Char('p')));
        assert_eq!((app.selected_file, app.selected().unwrap().cursor), (0, 5));
        app.update(Message::Key(Key::Escape));
        app.update(Message::Key(Key::Char('n')));
        assert_eq!(app.selected().unwrap().cursor, 5);

        app.files[0].cursor = 0;
        app.update(Message::Key(Key::Char('/')));
        app.update(Message::Key(Key::Char('T')));
        assert_eq!(app.selected().unwrap().cursor, 0);
        app.update(Message::Key(Key::Escape));
        app.update(Message::Key(Key::Char('/')));
        app.update(Message::Key(Key::Char('t')));
        assert_eq!(app.selected().unwrap().cursor, 5);
    }
}
