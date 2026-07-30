//! Pure review navigation and Ratatui rendering.

use std::ops::RangeInclusive;

use pr_core::diff::{DiffRow, NoticeKind};
use pr_core::excerpt::DiffExcerpt;
use pr_core::herdr::InsertResult;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use crate::review::{ReviewState, ReviewStatus, ReviewWarning};

const MIN_WIDTH: u16 = 40;
const MIN_HEIGHT: u16 = 6;
const NARROW_WIDTH: u16 = 72;

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
    Tab,
    Down,
    Up,
    First,
    Last,
    HalfPageDown,
    HalfPageUp,
    Visual,
    Escape,
    Enter,
    Space,
    Quit,
}

/// One file presented by the review state machine.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewFile {
    /// Escaped repository-relative display path.
    pub path: String,
    /// Current review state.
    pub status: ReviewStatus,
    rows: Vec<DiffRow>,
    cursor: usize,
    scroll: usize,
    visited: bool,
    loading: bool,
}

impl ReviewFile {
    /// Create one file summary before its diff arrives.
    pub fn new(path: impl Into<String>, status: ReviewStatus) -> Self {
        Self {
            path: path.into(),
            status,
            rows: Vec::new(),
            cursor: 0,
            scroll: 0,
            visited: false,
            loading: false,
        }
    }

    fn has_notice(&self) -> bool {
        self.rows
            .iter()
            .any(|row| matches!(row, DiffRow::Notice { .. }))
    }

    fn first_hunk(&self) -> usize {
        self.rows
            .iter()
            .position(|row| matches!(row, DiffRow::Hunk { .. }))
            .unwrap_or(0)
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
}

/// A typed result from keyboard input or asynchronous work.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Message {
    /// A complete repository poll.
    FilesLoaded {
        change_id: String,
        commit_id: String,
        files: Vec<ReviewFile>,
    },
    /// A diff result for an exact change and path.
    DiffLoaded {
        commit_id: String,
        path: String,
        rows: Vec<DiffRow>,
    },
    /// A diff command failed.
    DiffFailed {
        commit_id: String,
        path: String,
        message: String,
    },
    /// A review-state write result.
    ReviewFinished {
        change_id: String,
        path: String,
        result: Result<ReviewState, String>,
    },
    /// An excerpt insertion result.
    InsertFinished(Result<InsertResult, String>),
    /// A repository poll failed.
    PollFailed(String),
    /// The terminal size changed.
    Resize { height: u16 },
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
    /// Set the selected path review state.
    SetReviewed { path: String, reviewed: bool },
    /// Insert one valid unified-diff excerpt.
    Insert { text: String },
    /// Stop the application.
    Quit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Selection {
    anchor: usize,
    cursor: usize,
    fixed: bool,
}

impl Selection {
    fn range(self) -> RangeInclusive<usize> {
        self.anchor.min(self.cursor)..=self.anchor.max(self.cursor)
    }
}

/// The complete pure review UI state.
#[derive(Debug)]
pub struct ReviewApp {
    change_id: String,
    commit_id: String,
    files: Vec<ReviewFile>,
    selected_file: usize,
    file_scroll: usize,
    focus: Focus,
    selection: Option<Selection>,
    notice: Option<String>,
    review_in_flight: Option<bool>,
    height: u16,
}

impl Default for ReviewApp {
    fn default() -> Self {
        Self {
            change_id: String::new(),
            commit_id: String::new(),
            files: Vec::new(),
            selected_file: 0,
            file_scroll: 0,
            focus: Focus::Files,
            selection: None,
            notice: None,
            review_in_flight: None,
            height: 24,
        }
    }
}

impl ReviewApp {
    /// Apply one input and return any work for the I/O layer.
    pub fn update(&mut self, message: Message) -> Action {
        match message {
            Message::FilesLoaded {
                change_id,
                commit_id,
                files,
            } => self.load_files(change_id, commit_id, files),
            Message::DiffLoaded {
                commit_id,
                path,
                rows,
            } => {
                self.load_diff(&commit_id, &path, rows);
                Action::None
            }
            Message::DiffFailed {
                commit_id,
                path,
                message,
            } => {
                self.fail_diff(&commit_id, &path, message);
                Action::None
            }
            Message::ReviewFinished {
                change_id,
                path,
                result,
            } => {
                self.finish_review(&change_id, &path, result);
                Action::None
            }
            Message::InsertFinished(result) => {
                match result {
                    Ok(InsertResult::Inserted { agent_name }) => {
                        self.selection = None;
                        self.notice = Some(format!("Inserted into {agent_name}"));
                    }
                    Ok(InsertResult::NoAgent) => {
                        self.notice =
                            Some("No agent chat is available in this workspace".to_owned());
                    }
                    Err(message) => self.notice = Some(message),
                }
                Action::None
            }
            Message::PollFailed(message) => {
                self.notice = Some(message);
                Action::None
            }
            Message::Resize { height } => {
                self.height = height;
                self.keep_visible();
                Action::None
            }
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
                    file.visited = old.visited;
                    if same_snapshot {
                        file.rows.clone_from(&old.rows);
                        file.loading = old.loading;
                    }
                }
            }
        } else {
            self.selection = None;
            self.notice = None;
            self.file_scroll = 0;
            self.focus = Focus::Files;
        }

        self.change_id = change_id;
        self.commit_id = commit_id;
        self.files = files;
        let selected_file = selected_path
            .as_deref()
            .and_then(|path| self.files.iter().position(|file| file.path == path));
        self.selected_file = selected_file
            .unwrap_or(0)
            .min(self.files.len().saturating_sub(1));
        self.review_in_flight = None;
        if selected_file.is_none() {
            self.selection = None;
        }
        self.visit_selected();
        self.keep_visible();
        self.load_selected_action()
    }

    fn load_diff(&mut self, commit_id: &str, path: &str, rows: Vec<DiffRow>) {
        if self.commit_id != commit_id {
            return;
        }
        let Some(file) = self.files.iter_mut().find(|file| file.path == path) else {
            return;
        };
        file.rows = rows;
        file.loading = false;
        file.cursor = file.cursor.min(file.rows.len().saturating_sub(1));
        file.scroll = file.scroll.min(file.cursor);
        if self
            .selected()
            .is_some_and(|selected| selected.path == path)
        {
            self.selection = None;
            self.visit_selected();
            self.keep_visible();
        }
    }

    fn fail_diff(&mut self, commit_id: &str, path: &str, message: String) {
        if self.commit_id != commit_id {
            return;
        }
        if let Some(file) = self.files.iter_mut().find(|file| file.path == path) {
            file.loading = false;
            self.notice = Some(message);
        }
    }

    fn finish_review(&mut self, change_id: &str, path: &str, result: Result<ReviewState, String>) {
        if self.change_id != change_id {
            return;
        }
        self.review_in_flight = None;
        match result {
            Ok(state) => {
                if let Some(file) = self.files.iter_mut().find(|file| file.path == path) {
                    file.status = state.status;
                }
                self.notice = state.warning.map(Self::warning_text);
            }
            Err(message) => self.notice = Some(message),
        }
    }

    fn warning_text(warning: ReviewWarning) -> String {
        match warning {
            ReviewWarning::UnknownSchema => {
                "Review state uses an unknown schema; file is unreviewed".to_owned()
            }
            ReviewWarning::BaselineExpired => {
                "Review baseline expired; file reset to unreviewed".to_owned()
            }
        }
    }

    fn key(&mut self, key: Key) -> Action {
        match key {
            Key::Quit => Action::Quit,
            Key::Tab => {
                self.focus = match self.focus {
                    Focus::Files => Focus::Diff,
                    Focus::Diff => Focus::Files,
                };
                Action::None
            }
            Key::Escape => {
                self.selection = None;
                self.notice = None;
                Action::None
            }
            Key::Enter if self.focus == Focus::Files => {
                self.focus = Focus::Diff;
                Action::None
            }
            Key::Enter => self.insert(),
            Key::Space => self.toggle_review(),
            Key::Visual if self.focus == Focus::Diff => {
                self.visual();
                Action::None
            }
            Key::Visual => Action::None,
            Key::Down => self.navigate(1),
            Key::Up => self.navigate(-1),
            Key::First => self.navigate_to(0),
            Key::Last => {
                let last = self.focus_len().saturating_sub(1);
                self.navigate_to(last)
            }
            Key::HalfPageDown => self.navigate(self.half_page_rows()),
            Key::HalfPageUp => self.navigate(-self.half_page_rows()),
        }
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
            self.notice = Some("Select diff lines with v before insertion".to_owned());
            return Action::None;
        };
        let range = selection.range();
        if !range
            .clone()
            .any(|index| file.rows.get(index).is_some_and(Self::is_selectable_row))
        {
            self.notice = Some("Select context, added, or deleted lines".to_owned());
            return Action::None;
        }
        match DiffExcerpt::build(&file.rows, range) {
            Ok(excerpt) => Action::Insert {
                text: excerpt.into_string(),
            },
            Err(error) => {
                self.notice = Some(error.to_string());
                Action::None
            }
        }
    }

    fn visual(&mut self) {
        let Some(file) = self.selected() else {
            return;
        };
        if !file
            .rows
            .get(file.cursor)
            .is_some_and(Self::is_selectable_row)
        {
            self.notice = Some("This diff row cannot be selected".to_owned());
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

    fn is_selectable_row(row: &DiffRow) -> bool {
        matches!(
            row,
            DiffRow::Context { .. } | DiffRow::Delete { .. } | DiffRow::Add { .. }
        )
    }

    fn navigate(&mut self, delta: isize) -> Action {
        let current = match self.focus {
            Focus::Files => self.selected_file,
            Focus::Diff => self.selected().map_or(0, |file| file.cursor),
        };
        self.navigate_to(current.saturating_add_signed(delta))
    }

    fn navigate_to(&mut self, target: usize) -> Action {
        let mut selected_changed = false;
        match self.focus {
            Focus::Files => {
                let target = target.min(self.files.len().saturating_sub(1));
                if target != self.selected_file {
                    self.selected_file = target;
                    self.selection = None;
                    self.visit_selected();
                    selected_changed = true;
                }
            }
            Focus::Diff => {
                let Some(file) = self.files.get_mut(self.selected_file) else {
                    return Action::None;
                };
                file.cursor = target.min(file.rows.len().saturating_sub(1));
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

    fn visit_selected(&mut self) {
        let Some(file) = self.files.get_mut(self.selected_file) else {
            return;
        };
        if !file.visited && !file.rows.is_empty() {
            file.cursor = file.first_hunk();
            file.scroll = file.cursor;
            file.visited = true;
        }
    }

    fn load_selected_action(&mut self) -> Action {
        let commit_id = self.commit_id.clone();
        let Some(file) = self.files.get_mut(self.selected_file) else {
            return Action::None;
        };
        if !file.rows.is_empty() || file.loading {
            return Action::None;
        }
        file.loading = true;
        Action::LoadDiff {
            commit_id,
            path: file.path.clone(),
        }
    }

    fn keep_visible(&mut self) {
        let page = self.page_rows();
        if self.selected_file < self.file_scroll {
            self.file_scroll = self.selected_file;
        } else if self.selected_file >= self.file_scroll + page {
            self.file_scroll = self.selected_file + 1 - page;
        }
        let Some(file) = self.files.get_mut(self.selected_file) else {
            return;
        };
        if file.cursor < file.scroll {
            file.scroll = file.cursor;
        } else if file.cursor >= file.scroll + page {
            file.scroll = file.cursor + 1 - page;
        }
    }

    fn page_rows(&self) -> usize {
        let footer = usize::from(if self.height < 10 { 1_u16 } else { 2_u16 });
        usize::from(self.height)
            .saturating_sub(1 + footer + 2)
            .max(1)
    }

    fn half_page_rows(&self) -> isize {
        isize::try_from(self.page_rows().div_ceil(2)).unwrap_or(isize::MAX)
    }

    fn focus_len(&self) -> usize {
        match self.focus {
            Focus::Files => self.files.len(),
            Focus::Diff => self.selected().map_or(0, |file| file.rows.len()),
        }
    }

    fn selected(&self) -> Option<&ReviewFile> {
        self.files.get(self.selected_file)
    }
}

/// A side-effect-free view of [`ReviewApp`].
pub struct ReviewView<'a>(&'a ReviewApp);

impl Widget for ReviewView<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
            Paragraph::new("Terminal is too small\nMinimum: 40x6\nq quit").render(area, buffer);
            return;
        }

        let footer_height = if area.height < 10 { 1 } else { 2 };
        let header = Rect::new(area.x, area.y, area.width, 1);
        let body = Rect::new(
            area.x,
            area.y + 1,
            area.width,
            area.height - 1 - footer_height,
        );
        let footer = Rect::new(
            area.x,
            area.bottom() - footer_height,
            area.width,
            footer_height,
        );
        self.render_header(header, buffer);
        if area.width < NARROW_WIDTH {
            match self.0.focus {
                Focus::Files => self.render_files(body, buffer),
                Focus::Diff => self.render_diff(body, buffer),
            }
        } else {
            let file_width = (area.width * 30 / 100).clamp(24, 48);
            self.render_files(Rect::new(body.x, body.y, file_width, body.height), buffer);
            self.render_diff(
                Rect::new(
                    body.x + file_width,
                    body.y,
                    body.width - file_width,
                    body.height,
                ),
                buffer,
            );
        }
        self.render_footer(footer, buffer);
    }
}

impl ReviewView<'_> {
    fn render_header(&self, area: Rect, buffer: &mut Buffer) {
        let reviewed = self
            .0
            .files
            .iter()
            .filter(|file| file.status == ReviewStatus::Reviewed)
            .count();
        let change = self.0.change_id.chars().take(8).collect::<String>();
        Paragraph::new(format!(
            " Progressive review · change {change} · {reviewed}/{} reviewed",
            self.0.files.len()
        ))
        .render(area, buffer);
    }

    fn render_files(&self, area: Rect, buffer: &mut Buffer) {
        let focused = self.0.focus == Focus::Files;
        let block = Self::block("Files", focused);
        let inner = block.inner(area);
        block.render(area, buffer);
        let width = usize::from(inner.width.saturating_sub(3));
        let lines = self
            .0
            .files
            .iter()
            .enumerate()
            .skip(self.0.file_scroll)
            .take(usize::from(inner.height))
            .map(|(index, file)| {
                let style = if index == self.0.selected_file {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };
                Line::styled(
                    format!("{} {}", file.marker(), Self::shorten(&file.path, width)),
                    style,
                )
            })
            .collect::<Vec<_>>();
        Paragraph::new(lines).render(inner, buffer);
    }

    fn render_diff(&self, area: Rect, buffer: &mut Buffer) {
        let focused = self.0.focus == Focus::Diff;
        let title = self
            .0
            .selected()
            .map_or_else(|| "Diff".to_owned(), |file| format!("Diff · {}", file.path));
        let block = Self::block(&title, focused);
        let inner = block.inner(area);
        block.render(area, buffer);
        let Some(file) = self.0.selected() else {
            return;
        };
        let selection = self.0.selection.map(Selection::range);
        let lines = file
            .rows
            .iter()
            .enumerate()
            .skip(file.scroll)
            .take(usize::from(inner.height))
            .map(|(index, row)| {
                let mut style = Self::row_style(row);
                if selection
                    .as_ref()
                    .is_some_and(|selection| selection.contains(&index))
                {
                    style = style.bg(Color::DarkGray);
                }
                if focused && index == file.cursor {
                    style = style.add_modifier(Modifier::REVERSED);
                }
                Line::styled(Self::row_text(row, &file.path), style)
            })
            .collect::<Vec<_>>();
        Paragraph::new(lines).render(inner, buffer);
    }

    fn render_footer(&self, area: Rect, buffer: &mut Buffer) {
        let controls = "Tab focus · j/k move · v select · Enter insert · Space review · q quit";
        let status = match self.0.review_in_flight {
            Some(true) => "Marking reviewed…",
            Some(false) => "Removing review mark…",
            None => self.0.notice.as_deref().unwrap_or(""),
        };
        let lines = if area.height == 1 {
            vec![Line::from(vec![Span::raw(if status.is_empty() {
                controls
            } else {
                status
            })])]
        } else {
            vec![Line::raw(controls), Line::raw(status)]
        };
        Paragraph::new(lines).render(area, buffer);
    }

    fn block(title: &str, focused: bool) -> Block<'_> {
        let style = if focused {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };
        let suffix = if focused { " (focus)" } else { "" };
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" {title}{suffix} "))
            .border_style(style)
    }

    fn row_text(row: &DiffRow, path: &str) -> String {
        match row {
            DiffRow::FileHeader { .. } => format!("diff --git {path}"),
            DiffRow::Meta { text }
            | DiffRow::Context { text, .. }
            | DiffRow::Delete { text, .. }
            | DiffRow::Add { text, .. }
            | DiffRow::Notice { text, .. } => text.clone(),
            DiffRow::Hunk {
                old_start,
                old_count,
                new_start,
                new_count,
            } => format!("@@ -{old_start},{old_count} +{new_start},{new_count} @@"),
        }
    }

    fn row_style(row: &DiffRow) -> Style {
        let color = match row {
            DiffRow::Add { .. } => Color::Green,
            DiffRow::Delete { .. } => Color::Red,
            DiffRow::Hunk { .. } => Color::Cyan,
            DiffRow::Notice {
                kind: NoticeKind::Conflict | NoticeKind::Unsupported,
                ..
            } => Color::Yellow,
            DiffRow::Notice {
                kind: NoticeKind::Binary,
                ..
            } => Color::Magenta,
            _ => Color::Reset,
        };
        Style::default().fg(color)
    }

    fn shorten(value: &str, width: usize) -> String {
        let length = value.chars().count();
        if length <= width {
            return value.to_owned();
        }
        if width <= 3 {
            return ".".repeat(width);
        }
        let left = (width - 1) / 2;
        let right = width - left - 1;
        format!(
            "{}…{}",
            value.chars().take(left).collect::<String>(),
            value.chars().skip(length - right).collect::<String>()
        )
    }
}
