//! Changed-file list state, input handling, and rendering.

use std::collections::HashSet;

use component_core::{AnyInput, Component, ComponentSubscriptions, EventPublisher, InputScope};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};
use review_guide::ReviewCheckpoint;
use review_repository::repository::ChangeKind;
use review_state::{ReviewState, ReviewStatus};
use review_types::ReviewUnit;
use ui_actions::Action;
use ui_events::{
    FileDecorationsChanged, FileSelected, FileSelectionRequested, FileSummary,
    FilesOverviewChanged, FilesViewportChanged, GuidePathsChanged, PointerInput, PointerInputKind,
    RepositoryFilesChanged, ReviewStateSaved, ReviewableFiles, ReviewableFilesChanged,
    TemporaryFilesChanged,
};
use ui_shortcuts::{
    ApplicationShortcut, FileShortcut, NavigationShortcut, ShortcutCommand, ShortcutMatcher,
    ShortcutSet,
};
use ui_theme::Palette;
use unicode_width::UnicodeWidthStr;

mod tree;
use tree::{FileTree, FileTreeRow};
mod preview;
pub use preview::{FilePreviewList, PreviewFile};
mod badges;
use badges::FileBadges;

/// The complete changed-file pane component.
pub struct FilesComponent {
    events: EventPublisher,
    review_checkpoint: ReviewCheckpoint,
    associations: review_threads::ThreadPaths,
    files: Vec<FileSummary>,
    thread_paths: Vec<String>,
    tree: FileTree,
    collapsed_directories: HashSet<String>,
    selected: usize,
    scroll: usize,
    page_rows: usize,
    guide_paths: HashSet<String>,
    notice_paths: HashSet<String>,
    search_match_paths: HashSet<String>,
    pending_review: Option<PendingReview>,
    reviewable_files: ReviewableFiles,
    threads: Option<review_threads::ReviewThreads>,
    navigation: ui_events::ReviewNavigation,
}

struct PendingReview {
    path: String,
    previous_state: ReviewState,
    optimistic_state: ReviewState,
}

impl FilesComponent {
    #[cfg(test)]
    fn new(events: EventPublisher) -> Self {
        Self::with_reviewable_files(events, ReviewableFiles::default())
    }

    /// Create an empty files component with its shared read model.
    pub fn with_reviewable_files(
        events: EventPublisher,
        reviewable_files: ReviewableFiles,
    ) -> Self {
        Self {
            events,
            review_checkpoint: ReviewCheckpoint::new(ReviewUnit::default(), String::new()),
            files: Vec::new(),
            associations: review_threads::ThreadPaths::default(),
            thread_paths: Vec::new(),
            tree: FileTree::default(),
            collapsed_directories: HashSet::new(),
            selected: 0,
            scroll: 0,
            page_rows: 1,
            guide_paths: HashSet::new(),
            notice_paths: HashSet::new(),
            search_match_paths: HashSet::new(),
            pending_review: None,
            reviewable_files,
            threads: None,
            navigation: ui_events::ReviewNavigation::Files,
        }
    }

    /// Render the complete files pane from component-owned state.
    pub fn render(&self, area: Rect, buffer: &mut Buffer, palette: Palette, focused: bool) {
        let border_color = if focused { palette.focus } else { palette.dim };
        let focus_suffix = if focused { " (focus)" } else { "" };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" Files{focus_suffix} "))
            .border_style(Style::default().fg(border_color));
        let content_area = block.inner(area);
        block.render(area, buffer);
        let lines = self
            .tree
            .rows
            .iter()
            .skip(self.scroll)
            .take(usize::from(content_area.height))
            .map(|row| self.render_row(row, usize::from(content_area.width), palette))
            .collect::<Vec<_>>();
        Paragraph::new(lines).render(content_area, buffer);
    }

    fn selection(&self) -> Option<FileSelected> {
        self.selected_path().map(|path| FileSelected { path })
    }

    /// Return aggregate values for the repository header.
    fn overview(&self) -> FilesOverviewChanged {
        FilesOverviewChanged {
            reviewed: self
                .files
                .iter()
                .filter(|file| {
                    !file.temporary && file.review_state.status == ReviewStatus::Reviewed
                })
                .count(),
            total: self.files.iter().filter(|file| !file.temporary).count(),
            lines_added: self
                .files
                .iter()
                .map(|file| file.file.statistics.lines_added)
                .sum(),
            lines_removed: self
                .files
                .iter()
                .map(|file| file.file.statistics.lines_removed)
                .sum(),
        }
    }

    fn repository_changed(&mut self, event: &RepositoryFilesChanged) {
        let same_review_unit =
            self.review_checkpoint.review_unit == event.review_checkpoint.review_unit;
        let same_checkpoint = self.review_checkpoint == event.review_checkpoint;
        let previous_selected_path = same_review_unit.then(|| self.selected_path()).flatten();
        if !same_review_unit {
            self.threads = None;
            self.thread_paths.clear();
            self.collapsed_directories.clear();
            self.scroll = 0;
            self.pending_review = None;
        }
        if !same_review_unit || !same_checkpoint {
            self.guide_paths.clear();
        }
        if same_review_unit {
            let paths_to_expand = event
                .files
                .iter()
                .filter(|file| {
                    self.files.iter().any(|previous| {
                        previous.file.review_path() == file.file.review_path()
                            && needs_parent_expansion(
                                file.review_state.status,
                                previous.review_state.status,
                            )
                    })
                })
                .map(FileSummary::path)
                .collect::<Vec<_>>();
            for path in paths_to_expand {
                self.expand_file_parents(&path);
            }
        }
        self.review_checkpoint.clone_from(&event.review_checkpoint);
        self.files.clone_from(&event.files);
        self.associations = ui_events::FileSummary::thread_paths(&self.files);
        self.add_thread_files();
        if let Some(pending) = &self.pending_review
            && let Some(file) = self
                .files
                .iter_mut()
                .find(|file| file.path() == pending.path)
        {
            file.review_state = pending.optimistic_state;
        }
        self.selected = previous_selected_path
            .as_deref()
            .and_then(|path| self.files.iter().position(|file| file.path() == path))
            .unwrap_or(0)
            .min(self.files.len().saturating_sub(1));
        self.rebuild_tree();
        self.refresh_reviewable_files();
        self.keep_selected_visible();
        self.publish_selection_if_changed(previous_selected_path.as_deref());
        self.publish_overview();
    }

    fn threads_loaded(&mut self, event: &ui_events::ReviewThreadsLoaded) {
        if event.review_unit == self.review_checkpoint.review_unit
            && let Ok(book) = &event.result
        {
            self.threads = Some(book.clone());
        }
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn navigation_changed(&mut self, event: &ui_events::ReviewNavigationChanged) {
        self.navigation = event.0;
    }

    fn review_state_saved(&mut self, event: &ReviewStateSaved) {
        if event.review_unit != self.review_checkpoint.review_unit {
            return;
        }
        let pending = match self.pending_review.as_ref() {
            Some(pending) if pending.path == event.path => self.pending_review.take(),
            Some(_) => return,
            None => None,
        };
        let Ok(review_state) = event.result else {
            if let Some(pending) = pending
                && let Some(file) = self.files.iter_mut().find(|file| file.path() == event.path)
            {
                file.review_state = pending.previous_state;
            }
            self.refresh_reviewable_files();
            self.publish_overview();
            return;
        };
        let Some(file) = self.files.iter_mut().find(|file| file.path() == event.path) else {
            return;
        };
        let expand = needs_parent_expansion(review_state.status, file.review_state.status);
        file.review_state = review_state;
        self.reviewed_notice(&event.path, review_state.status);
        self.refresh_reviewable_files();
        if expand {
            self.expand_file_parents(&event.path);
            self.rebuild_tree();
        }
        self.publish_overview();
    }

    fn temporary_files_changed(&mut self, event: &TemporaryFilesChanged) {
        let current_temporary_files = self
            .files
            .iter()
            .filter(|file| file.temporary)
            .cloned()
            .collect::<Vec<_>>();
        if current_temporary_files == event.files {
            return;
        }
        let selected_path = self.selected_path();
        self.files.retain(|file| !file.temporary);
        self.files.extend(event.files.iter().cloned());
        self.add_thread_files();
        self.selected = selected_path
            .as_deref()
            .and_then(|path| self.files.iter().position(|file| file.path() == path))
            .unwrap_or(0)
            .min(self.files.len().saturating_sub(1));
        self.rebuild_tree();
        self.keep_selected_visible();
        self.publish_selection_if_changed(selected_path.as_deref());
    }

    fn reviewed_notice(&self, path: &str, status: ReviewStatus) {
        if status != ReviewStatus::Reviewed {
            return;
        }
        let open = self.threads.as_ref().map_or(0, |book| {
            book.counts_for(|thread| self.current_thread_path(thread.path()) == path)
                .open
        });
        if open > 0 {
            self.events.publish(ui_events::ToastRequested {
                text: format!("File reviewed. {open} open threads remain."),
                kind: toasts::ToastKind::Info,
            });
        }
    }

    fn add_thread_files(&mut self) {
        for path in &self.thread_paths {
            if !self.files.iter().any(|file| file.path() == *path) {
                self.files.push(FileSummary::temporary(
                    path,
                    format!("{path} · threads"),
                    std::path::PathBuf::from(path),
                ));
            }
        }
    }

    fn thread_files_changed(&mut self, event: &ui_events::ThreadFilesChanged) {
        if self.thread_paths == event.paths {
            return;
        }
        let selected = self.selected_path();
        self.files
            .retain(|file| !file.temporary || !self.thread_paths.contains(&file.path()));
        self.thread_paths.clone_from(&event.paths);
        self.add_thread_files();
        self.selected = selected
            .as_ref()
            .and_then(|path| self.files.iter().position(|file| file.path() == *path))
            .unwrap_or(0);
        self.rebuild_tree();
        self.keep_selected_visible();
        self.publish_selection_if_changed(selected.as_deref());
    }

    fn guide_paths_changed(&mut self, event: &GuidePathsChanged) {
        self.guide_paths = event.paths.iter().cloned().collect();
    }

    fn decorations_changed(&mut self, event: &FileDecorationsChanged) {
        self.notice_paths = event.notice_paths.iter().cloned().collect();
        self.search_match_paths = event.search_match_paths.iter().cloned().collect();
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn viewport_changed(&mut self, event: &FilesViewportChanged) {
        self.page_rows = event.rows.max(1);
        self.keep_selected_visible();
    }

    fn selection_requested(&mut self, event: &FileSelectionRequested) {
        let previous_selected_path = self.selected_path();
        let Some(selected) = self.files.iter().position(|file| file.path() == event.path) else {
            return;
        };
        self.selected = selected;
        self.keep_selected_visible();
        self.publish_selection_if_changed(previous_selected_path.as_deref());
    }

    fn shortcut(&mut self, shortcut: ShortcutCommand) -> Vec<Action> {
        if self.navigation == ui_events::ReviewNavigation::Threads {
            return Vec::new();
        }
        let previous_selected_path = self.selected_path();
        let actions = match shortcut {
            ShortcutCommand::Application(ApplicationShortcut::MarkReviewed) => {
                self.toggle_review().into_iter().collect()
            }
            ShortcutCommand::File(shortcut) => {
                self.move_to_unreviewed_file(shortcut);
                Vec::new()
            }
            ShortcutCommand::Navigation(navigation) => {
                self.move_selection(navigation);
                Vec::new()
            }
            _ => Vec::new(),
        };
        self.publish_selection_if_changed(previous_selected_path.as_deref());
        actions
    }

    fn move_to_unreviewed_file(&mut self, shortcut: FileShortcut) {
        if self.files.is_empty() {
            return;
        }
        let is_unreviewed = |index: &usize| {
            let file = &self.files[*index];
            !file.temporary && file.review_state.status.needs_review()
        };
        let target = match shortcut {
            FileShortcut::GoToNextUnreviewed => (self.selected.saturating_add(1)..self.files.len())
                .find(is_unreviewed)
                .or_else(|| (0..=self.selected).find(is_unreviewed)),
            FileShortcut::GoToPreviousUnreviewed => (0..self.selected)
                .rev()
                .find(is_unreviewed)
                .or_else(|| (self.selected..self.files.len()).rev().find(is_unreviewed)),
        };
        let Some(target) = target else {
            return;
        };
        self.selected = target;
        let path = self.files[target].path();
        self.expand_file_parents(&path);
        self.rebuild_tree();
        self.keep_selected_visible();
    }

    fn pointer_input(&mut self, input: PointerInput) -> Vec<Action> {
        if let PointerInputKind::Scroll(delta) = input.kind {
            self.scroll_input(delta);
            return Vec::new();
        }
        let double_click = match input.kind {
            PointerInputKind::Click | PointerInputKind::ControlClick => false,
            PointerInputKind::DoubleClick => true,
            PointerInputKind::Scroll(_)
            | PointerInputKind::RightClick
            | PointerInputKind::Drag
            | PointerInputKind::Release => return Vec::new(),
        };
        let Some(position) = input.position else {
            return Vec::new();
        };
        let previous_selected_path = self.selected_path();
        let row = self
            .scroll
            .saturating_add(usize::from(position.component_row));
        if let Some(file) = self.tree.file_at(row) {
            self.selected = file;
            self.keep_selected_visible();
            if double_click {
                self.publish_selection_if_changed(previous_selected_path.as_deref());
                return self.shortcut(ShortcutCommand::Application(
                    ApplicationShortcut::MarkReviewed,
                ));
            }
            self.publish_selection_if_changed(previous_selected_path.as_deref());
            return Vec::new();
        }
        let Some(FileTreeRow::Directory { depth, path, .. }) = self.tree.rows.get(row) else {
            return Vec::new();
        };
        let expected_column = u16::try_from(depth.saturating_mul(2)).unwrap_or(u16::MAX);
        if position.component_column != expected_column {
            return Vec::new();
        }
        let path = path.clone();
        self.toggle_directory(path);
        self.publish_selection_if_changed(previous_selected_path.as_deref());
        Vec::new()
    }

    fn scroll_input(&mut self, delta: isize) {
        let previous_selected_path = self.selected_path();
        let steps = delta.unsigned_abs();
        let direction = if delta < 0 {
            NavigationShortcut::MoveUp
        } else {
            NavigationShortcut::MoveDown
        };
        for _ in 0..steps {
            self.move_selection(direction);
        }
        self.publish_selection_if_changed(previous_selected_path.as_deref());
    }

    fn publish_selection_if_changed(&self, previous_path: Option<&str>) {
        if previous_path != self.selected_path().as_deref()
            && let Some(selection) = self.selection()
        {
            self.events.publish(selection);
        }
    }

    fn move_selection(&mut self, input: NavigationShortcut) {
        self.selected = self
            .tree
            .navigate(self.selected, input, self.page_rows)
            .unwrap_or(self.selected);
        self.keep_selected_visible();
    }

    fn toggle_review(&mut self) -> Option<Action> {
        if self.pending_review.is_some() {
            return None;
        }
        let file = self.files.get_mut(self.selected)?;
        if file.temporary {
            return None;
        }
        let path = file.path();
        let previous_state = file.review_state;
        let reviewed = previous_state.status.needs_review();
        let optimistic_state = if reviewed {
            ReviewState::reviewed()
        } else {
            ReviewState::unreviewed(file.file.statistics, None)
        };
        file.review_state = optimistic_state;
        self.refresh_reviewable_files();
        let next_file = reviewed
            .then(|| {
                self.tree
                    .visible_files()
                    .skip_while(|file| *file != self.selected)
                    .skip(1)
                    .find(|file| self.files[*file].review_state.status.needs_review())
            })
            .flatten();
        if let Some(next_file) = next_file {
            self.selected = next_file;
            self.keep_selected_visible();
        }
        self.pending_review = Some(PendingReview {
            path: path.clone(),
            previous_state,
            optimistic_state,
        });
        Some(Action::SetReviewed { path, reviewed })
    }

    fn selected_path(&self) -> Option<String> {
        self.files.get(self.selected).map(FileSummary::path)
    }

    fn refresh_reviewable_files(&self) {
        if self.reviewable_files.replace(
            self.files
                .iter()
                .filter(|file| !file.temporary && file.review_state.status.needs_review())
                .map(FileSummary::path)
                .collect(),
        ) {
            self.events.publish(ReviewableFilesChanged);
        }
    }

    fn publish_overview(&self) {
        self.events.publish(self.overview());
    }

    fn rebuild_tree(&mut self) {
        self.tree = FileTree::new(
            self.files
                .iter()
                .map(|file| (file.path(), file.display_path().to_owned())),
            &self.collapsed_directories,
        );
    }

    fn toggle_directory(&mut self, path: String) {
        if !self.collapsed_directories.remove(&path) {
            self.collapsed_directories.insert(path);
        }
        self.rebuild_tree();
        if self.tree.row_for_file(self.selected).is_none()
            && let Some(file) = self.tree.nearest_visible_file(self.selected)
        {
            self.selected = file;
            self.keep_selected_visible();
        }
    }

    fn expand_file_parents(&mut self, path: &str) {
        for (index, _) in path.match_indices('/') {
            self.collapsed_directories.remove(&path[..index]);
        }
    }

    fn keep_selected_visible(&mut self) {
        let Some(row) = self.tree.row_for_file(self.selected) else {
            self.scroll = 0;
            return;
        };
        if self.tree.visible_files().next() == Some(self.selected) {
            self.scroll = row.saturating_add(1).saturating_sub(self.page_rows);
            return;
        }
        if row < self.scroll {
            self.scroll = row;
        } else if row >= self.scroll.saturating_add(self.page_rows) {
            self.scroll = row.saturating_add(1).saturating_sub(self.page_rows);
        }
    }

    fn render_row(&self, row: &FileTreeRow, width: usize, palette: Palette) -> Line<'static> {
        match row {
            FileTreeRow::Directory {
                depth,
                name,
                collapsed,
                ..
            } => {
                let label = format!(
                    "{}{} {name}/",
                    "  ".repeat(*depth),
                    if *collapsed { '▸' } else { '▾' }
                );
                Line::styled(
                    shorten(&label, width),
                    Style::default()
                        .fg(palette.dim)
                        .add_modifier(Modifier::BOLD),
                )
            }
            FileTreeRow::File { depth, name, file } => {
                self.render_file(*depth, name, *file, width, palette)
            }
        }
    }

    fn render_file(
        &self,
        depth: usize,
        name: &str,
        index: usize,
        width: usize,
        palette: Palette,
    ) -> Line<'static> {
        let file = &self.files[index];
        let path = file.path();
        let marker = if self.notice_paths.contains(&path) {
            "!"
        } else {
            match file.review_state.status {
                ReviewStatus::Unreviewed => "○",
                ReviewStatus::Reviewed => "✓",
                ReviewStatus::ChangedSinceReview => "●",
            }
        };
        let prefix = format!("{}{} ", "  ".repeat(depth), marker);
        let counts = self
            .threads
            .as_ref()
            .map_or_else(review_threads::ThreadCounts::default, |book| {
                book.counts_for(|thread| self.current_thread_path(thread.path()) == path)
            });
        let badges = FileBadges {
            guide: self.guide_paths.contains(&path),
            threads: counts,
        }
        .line(palette);
        let statistics = FileStatistics::new(file);
        let prefix = shorten(
            &prefix,
            width.saturating_sub(badges.width() + statistics.width()),
        );
        let reserved =
            UnicodeWidthStr::width(prefix.as_str()) + badges.width() + statistics.width();
        let name = shorten(name, width.saturating_sub(reserved));
        let padding = width.saturating_sub(
            UnicodeWidthStr::width(prefix.as_str())
                + UnicodeWidthStr::width(name.as_str())
                + badges.width()
                + statistics.width(),
        );
        let color = file_color(file, palette);
        let mut spans = vec![
            Span::styled(prefix, Style::default().fg(color)),
            Span::styled(name, Style::default().fg(color)),
        ];
        spans.extend(badges.spans);
        spans.push(Span::raw(" ".repeat(padding)));
        statistics.append(&mut spans, palette);
        let mut style = if index == self.selected {
            Style::default().bg(palette.cursor)
        } else {
            Style::default()
        };
        if self.search_match_paths.contains(&path) {
            style = style.add_modifier(Modifier::BOLD);
        }
        Line::from(spans).style(style)
    }

    fn current_thread_path<'a>(&'a self, path: &'a str) -> &'a str {
        self.associations.resolve(path)
    }
}

fn needs_parent_expansion(status: ReviewStatus, previous: ReviewStatus) -> bool {
    status != previous && status != ReviewStatus::Reviewed
}

impl Component<Action> for FilesComponent {
    fn register_subscriptions(subscriptions: &mut ComponentSubscriptions<'_, Self, Action>) {
        subscriptions.subscribe(Self::threads_loaded);
        subscriptions.subscribe(Self::navigation_changed);
        subscriptions.subscribe(Self::repository_changed);
        subscriptions.subscribe(Self::review_state_saved);
        subscriptions.subscribe(Self::temporary_files_changed);
        subscriptions.subscribe(Self::thread_files_changed);
        subscriptions.subscribe(Self::guide_paths_changed);
        subscriptions.subscribe(Self::decorations_changed);
        subscriptions.subscribe(Self::viewport_changed);
        subscriptions.subscribe(Self::selection_requested);
        subscriptions.subscribe_input(
            InputScope::Focused,
            ShortcutMatcher::new(ShortcutSet::Files),
            Self::shortcut,
        );
        subscriptions.subscribe_input(
            InputScope::Global,
            ShortcutMatcher::new(ShortcutSet::FilesGlobal),
            Self::shortcut,
        );
        subscriptions.subscribe_input(InputScope::Hovered, AnyInput, Self::pointer_input);
    }
}

struct FileStatistics {
    added: Option<String>,
    removed: Option<String>,
}

impl FileStatistics {
    fn new(file: &FileSummary) -> Self {
        Self {
            added: (file.review_state.current_diff_statistics.lines_added > 0)
                .then(|| format!("+{}", file.review_state.current_diff_statistics.lines_added)),
            removed: (file.review_state.current_diff_statistics.lines_removed > 0).then(|| {
                format!(
                    "-{}",
                    file.review_state.current_diff_statistics.lines_removed
                )
            }),
        }
    }

    fn width(&self) -> usize {
        self.added.as_ref().map_or(0, String::len)
            + self.removed.as_ref().map_or(0, String::len)
            + usize::from(self.added.is_some() && self.removed.is_some())
    }

    fn append(self, spans: &mut Vec<Span<'static>>, palette: Palette) {
        let has_added = self.added.is_some();
        if let Some(added) = self.added {
            spans.push(Span::styled(added, Style::default().fg(palette.insertion)));
        }
        if let Some(removed) = self.removed {
            if has_added {
                spans.push(Span::raw(" "));
            }
            spans.push(Span::styled(removed, Style::default().fg(palette.deletion)));
        }
    }
}

fn file_color(file: &FileSummary, palette: Palette) -> Color {
    if file.temporary {
        return palette.dim;
    }
    match file.file.change {
        ChangeKind::Added => palette.insertion,
        ChangeKind::Deleted => palette.deletion,
        ChangeKind::Modified => palette.focus,
        ChangeKind::Renamed | ChangeKind::TypeChanged | ChangeKind::Conflict => palette.warning,
    }
}

fn shorten(text: &str, width: usize) -> String {
    if UnicodeWidthStr::width(text) <= width {
        return text.to_owned();
    }
    if width <= 1 {
        return "…".chars().take(width).collect();
    }
    let mut result = String::new();
    let mut used: usize = 0;
    for character in text.chars() {
        let character_width = UnicodeWidthStr::width(character.to_string().as_str());
        if used.saturating_add(character_width) >= width {
            break;
        }
        result.push(character);
        used += character_width;
    }
    result.push('…');
    result
}

#[cfg(test)]
#[path = "lib.tests.rs"]
mod tests;
