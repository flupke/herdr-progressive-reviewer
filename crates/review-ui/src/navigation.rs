use std::path::{Path, PathBuf};

use crate::app::{Action, Focus, Key, LocationList, ReviewApp, ReviewFile, SourceLoadMode};
use crate::presentation::DiffPresentation;
use review_lsp::{Operation, Query, SourceLocation};
use review_types::ReviewUnit;
use toasts::ToastKind;

use crate::presentation::PresentationLocation;

const MAXIMUM_JUMP_COUNT: usize = 100;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum ReviewLocation {
    ReviewFile {
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

#[derive(Clone, Debug, Default)]
pub(super) struct LocationHistory {
    older: Vec<ReviewLocation>,
    newer: Vec<ReviewLocation>,
}

#[derive(Clone, Copy)]
enum LocationHistoryDirection {
    Previous,
    Next,
}

impl LocationHistory {
    pub(super) fn record_jump(&mut self, origin: ReviewLocation, target: &ReviewLocation) -> bool {
        if origin.same_line(target) {
            return false;
        }
        self.older.extend(self.newer.drain(..).rev());
        self.older.retain(|location| !location.same_line(&origin));
        self.older.push(origin);
        if self.older.len() > MAXIMUM_JUMP_COUNT {
            self.older.remove(0);
        }
        true
    }

    fn location(
        &mut self,
        direction: LocationHistoryDirection,
        current: ReviewLocation,
        is_restorable: impl Fn(&ReviewLocation) -> bool,
    ) -> Option<ReviewLocation> {
        match direction {
            LocationHistoryDirection::Previous => {
                Self::traverse(&mut self.older, &mut self.newer, current, is_restorable)
            }
            LocationHistoryDirection::Next => {
                Self::traverse(&mut self.newer, &mut self.older, current, is_restorable)
            }
        }
    }

    fn traverse(
        source: &mut Vec<ReviewLocation>,
        destination: &mut Vec<ReviewLocation>,
        current: ReviewLocation,
        is_restorable: impl Fn(&ReviewLocation) -> bool,
    ) -> Option<ReviewLocation> {
        while let Some(target) = source.pop() {
            if is_restorable(&target) {
                if is_restorable(&current) {
                    destination.push(current);
                }
                return Some(target);
            }
        }
        None
    }
}

impl ReviewLocation {
    pub(super) fn review_unit(&self) -> &ReviewUnit {
        match self {
            Self::ReviewFile { review_unit, .. }
            | Self::Source { review_unit, .. }
            | Self::Revision { review_unit } => review_unit,
        }
    }

    pub(super) fn for_review_unit(&self, review_unit: impl Into<ReviewUnit>) -> Self {
        let review_unit = review_unit.into();
        match self {
            Self::ReviewFile {
                path,
                cursor,
                presentation_location,
                column,
                ..
            } => Self::ReviewFile {
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

    fn is_restorable(
        &self,
        current_review_unit: &ReviewUnit,
        files: &[ReviewFile],
        repository_root: &Path,
    ) -> bool {
        if self.review_unit() != current_review_unit {
            return true;
        }
        match self {
            Self::ReviewFile { path, .. } => files
                .iter()
                .any(|file| file.path == *path && file.status.needs_review()),
            Self::Source { location, .. } => location
                .review_path(repository_root)
                .and_then(|path| files.iter().find(|file| file.path == path))
                .is_none_or(|file| file.status.needs_review()),
            Self::Revision { .. } => true,
        }
    }

    fn same_line(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::ReviewFile {
                    review_unit,
                    path,
                    cursor,
                    presentation_location,
                    ..
                },
                Self::ReviewFile {
                    review_unit: other_review_unit,
                    path: other_path,
                    cursor: other_cursor,
                    presentation_location: other_presentation_location,
                    ..
                },
            ) => {
                review_unit == other_review_unit
                    && path == other_path
                    && match (presentation_location, other_presentation_location) {
                        (Some(location), Some(other_location)) => location == other_location,
                        (None, None) => cursor == other_cursor,
                        (Some(_), None) | (None, Some(_)) => false,
                    }
            }
            (
                Self::Source {
                    review_unit,
                    location,
                },
                Self::Source {
                    review_unit: other_review_unit,
                    location: other_location,
                },
            ) => {
                review_unit == other_review_unit
                    && location.path == other_location.path
                    && location.line == other_location.line
            }
            (
                Self::Revision { review_unit },
                Self::Revision {
                    review_unit: other_review_unit,
                },
            ) => review_unit == other_review_unit,
            _ => false,
        }
    }
}

impl ReviewApp {
    pub(super) fn current_review_location(&self) -> Option<ReviewLocation> {
        let Some(file) = self.selected() else {
            return (!self.review_unit.is_empty()).then(|| ReviewLocation::Revision {
                review_unit: self.review_unit.clone(),
            });
        };
        if file.temporary {
            return file
                .cursor_location()
                .map(|location| ReviewLocation::Source {
                    review_unit: self.review_unit.clone(),
                    location,
                });
        }
        Some(ReviewLocation::ReviewFile {
            review_unit: self.review_unit.clone(),
            path: file.path.clone(),
            cursor: file.cursor,
            presentation_location: file.diff.presentation_location(file.cursor),
            column: file.column,
        })
    }

    pub(super) fn record_location_change(&mut self, origin: Option<ReviewLocation>) -> bool {
        let Some(target) = self.current_review_location() else {
            return false;
        };
        self.record_jump(origin, &target)
    }

    fn record_jump(&mut self, origin: Option<ReviewLocation>, target: &ReviewLocation) -> bool {
        let Some(origin) = origin else {
            return false;
        };
        if !origin.is_restorable(&self.review_unit, &self.files, &self.repository_root) {
            return false;
        }
        self.location_history.record_jump(origin, target)
    }

    pub(super) fn jump(&mut self, navigate: impl FnOnce(&mut Self) -> Action) -> Action {
        let origin = self.current_review_location();
        let action = navigate(self);
        if self.record_location_change(origin) {
            self.center_selected_location();
        }
        action
    }

    pub(super) fn center_selected_location(&mut self) {
        let page = self.page_rows();
        if let Some(file) = self.files.get_mut(self.selected_file) {
            file.jump_to_row(file.cursor, page);
        }
        self.keep_visible();
    }

    pub(super) fn load_locations(
        &mut self,
        operation: Operation,
        snapshot_id: &str,
        locations: Vec<SourceLocation>,
    ) -> Action {
        if snapshot_id != self.commit_id {
            return Action::None;
        }
        match locations.as_slice() {
            [] => {
                self.toasts.push("No locations found", ToastKind::Info);
                Action::None
            }
            [location] if operation == Operation::Definition => {
                self.accept_location(location.clone())
            }
            _ => {
                let location = locations[0].clone();
                self.locations = Some(LocationList {
                    operation,
                    locations,
                    selected: 0,
                    scroll: 0,
                    origin_file: self.selected_file,
                });
                self.preview = None;
                self.focus = Focus::Files;
                self.preview_location(location)
            }
        }
    }

    pub(super) fn accept_location(&mut self, location: SourceLocation) -> Action {
        let origin = self.current_review_location();
        let action = self.show_source_location(location.clone());
        if matches!(action, Action::LoadSource { .. }) {
            let _ = self.record_jump(
                origin,
                &ReviewLocation::Source {
                    review_unit: self.review_unit.clone(),
                    location,
                },
            );
        } else {
            let _ = self.record_location_change(origin);
        }
        action
    }

    fn show_source_location(&mut self, location: SourceLocation) -> Action {
        self.locations = None;
        self.preview = None;
        if let Some(index) = self.location_file_index(&location) {
            let path = self.files[index].path.clone();
            self.remove_temporary_files();
            let target = self
                .location_file_index(&location)
                .unwrap_or(index.min(self.files.len().saturating_sub(1)));
            self.select_file(target);
            self.focus = Focus::Diff;
            self.files[target].disk_path = Some(location.path.clone());
            self.expand_file_parents(&path);
            self.rebuild_file_tree();
            if self.files[target].diff.is_empty() {
                let page = self.page_rows();
                let _ = self.files[target].reveal_location(&location, page);
                return self.load_selected_action();
            }
            self.reveal_location(&location);
            return Action::None;
        }
        Action::LoadSource {
            snapshot_id: self.commit_id.clone(),
            location,
            mode: SourceLoadMode::External,
        }
    }

    pub(super) fn previous_location(&mut self) -> Action {
        self.navigate_location_history(LocationHistoryDirection::Previous)
    }

    pub(super) fn next_location(&mut self) -> Action {
        self.navigate_location_history(LocationHistoryDirection::Next)
    }

    fn navigate_location_history(&mut self, direction: LocationHistoryDirection) -> Action {
        let Some(current) = self.current_review_location() else {
            return Action::None;
        };
        let history_before = self.location_history.clone();
        let files = &self.files;
        let repository_root = &self.repository_root;
        let review_unit = &self.review_unit;
        let Some(location) = self
            .location_history
            .location(direction, current, |location| {
                location.is_restorable(review_unit, files, repository_root)
            })
        else {
            return Action::None;
        };
        if location.review_unit() != &self.review_unit {
            return self.edit_historical_revision(location, history_before);
        }
        self.show_review_location(location)
    }

    pub(super) fn show_review_location(&mut self, location: ReviewLocation) -> Action {
        let (path, cursor, presentation_location, column) = match location {
            ReviewLocation::ReviewFile {
                path,
                cursor,
                presentation_location,
                column,
                ..
            } => (path, cursor, presentation_location, column),
            ReviewLocation::Source { location, .. } => {
                return self.show_source_location(location);
            }
            ReviewLocation::Revision { .. } => return Action::None,
        };
        self.locations = None;
        self.preview = None;
        self.remove_temporary_files();
        let Some(target) = self.files.iter().position(|file| file.path == path) else {
            return Action::None;
        };
        self.select_file(target);
        self.focus = Focus::Diff;
        self.files[target].restore_presentation_location(presentation_location, cursor, column);
        self.expand_file_parents(&path);
        self.rebuild_file_tree();
        self.center_selected_location();
        self.load_selected_action()
    }

    pub(super) fn load_source(
        &mut self,
        snapshot_id: &str,
        location: &SourceLocation,
        content: &[u8],
        mode: SourceLoadMode,
    ) -> Action {
        if snapshot_id != self.commit_id {
            return Action::None;
        }
        let review_path = location.review_path(&self.repository_root);
        let display_path = if mode.external() {
            location.path.display().to_string()
        } else {
            location.display_path(&self.repository_root)
        };
        let highlighted =
            self.highlighter
                .highlight(&display_path, Vec::new(), None, Some(content));
        let mut diff = DiffPresentation::new(highlighted);
        let _ = diff.show_file();
        let mut file = ReviewFile::from_source(location, review_path.as_deref(), diff, mode);
        let _ = file.reveal_location(location, self.page_rows());
        if mode.external() {
            let path = file.path.clone();
            self.remove_temporary_files();
            self.files.push(file);
            let target = self
                .files
                .iter()
                .position(|file| file.disk_path.as_deref() == Some(location.path.as_path()))
                .unwrap_or(0);
            self.select_file(target);
            self.expand_file_parents(&path);
            self.rebuild_file_tree();
            self.focus = Focus::Diff;
            self.locations = None;
            self.preview = None;
            self.keep_visible();
        } else if self
            .locations
            .as_ref()
            .is_some_and(|list| list.locations.get(list.selected) == Some(location))
        {
            self.preview = Some(file);
            self.keep_visible();
        }
        if self.revision_source_waits_for(location) {
            self.finish_revision_location_restore();
        }
        Action::None
    }

    pub(super) fn reveal_location(&mut self, location: &SourceLocation) {
        let page = self.page_rows();
        if self
            .files
            .get_mut(self.selected_file)
            .is_some_and(|file| file.reveal_location(location, page))
        {
            self.keep_visible();
        }
    }

    pub(super) fn load_external_location(&mut self, location: SourceLocation) -> Action {
        Action::LoadSource {
            snapshot_id: self.commit_id.clone(),
            location,
            mode: SourceLoadMode::External,
        }
    }
}

impl ReviewApp {
    pub(super) fn location_list_key(&mut self, key: Key) -> Action {
        match key {
            Key::Escape => {
                let origin = self.locations.take().map_or(0, |list| list.origin_file);
                self.preview = None;
                self.select_file(origin.min(self.files.len().saturating_sub(1)));
                self.focus = Focus::Diff;
                self.keep_visible();
                Action::None
            }
            Key::Enter => self
                .locations
                .as_ref()
                .and_then(|list| list.locations.get(list.selected))
                .cloned()
                .map_or(Action::None, |location| self.accept_location(location)),
            Key::Down | Key::Char('j') => self.move_location(1),
            Key::Up | Key::Char('k') => self.move_location(-1),
            Key::First | Key::Char('g') => self.move_location_to(0),
            Key::Last | Key::Char('G') => {
                let last = self
                    .locations
                    .as_ref()
                    .map_or(0, |list| list.locations.len().saturating_sub(1));
                self.move_location_to(last)
            }
            _ => Action::None,
        }
    }

    pub(super) fn move_location(&mut self, delta: isize) -> Action {
        let current = self.locations.as_ref().map_or(0, |list| list.selected);
        self.move_location_to(current.saturating_add_signed(delta))
    }

    pub(super) fn move_location_to(&mut self, target: usize) -> Action {
        let page = self.page_rows();
        let Some(list) = &mut self.locations else {
            return Action::None;
        };
        list.selected = target.min(list.locations.len().saturating_sub(1));
        if list.selected < list.scroll {
            list.scroll = list.selected;
        } else if list.selected >= list.scroll + page {
            list.scroll = list.selected + 1 - page;
        }
        let location = list.locations[list.selected].clone();
        self.preview_location(location)
    }

    pub(super) fn preview_location(&mut self, location: SourceLocation) -> Action {
        let Some(index) = self.location_file_index(&location) else {
            return Action::LoadSource {
                snapshot_id: self.commit_id.clone(),
                location,
                mode: SourceLoadMode::Preview,
            };
        };
        self.preview = None;
        if self.files[index].diff.is_empty() && !self.files[index].diff.can_show_file() {
            let commit_id = self.commit_id.clone();
            return self.files[index]
                .start_diff_load()
                .map_or(Action::None, |path| Action::LoadDiff { commit_id, path });
        }
        let mut preview = self.files[index].clone();
        preview.disk_path = Some(location.path.clone());
        let _ = preview.reveal_location(&location, self.page_rows());
        self.preview = Some(preview);
        self.keep_visible();
        Action::None
    }

    pub(super) fn context_menu_key(&mut self, key: Key) -> Action {
        match key {
            Key::Escape => {
                self.context_menu = None;
                Action::None
            }
            Key::Down | Key::Char('j') => {
                if let Some(menu) = &mut self.context_menu {
                    menu.selected = (menu.selected + 1).min(2);
                }
                Action::None
            }
            Key::Up | Key::Char('k') => {
                if let Some(menu) = &mut self.context_menu {
                    menu.selected = menu.selected.saturating_sub(1);
                }
                Action::None
            }
            Key::Enter => {
                let operation = self
                    .context_menu
                    .take()
                    .filter(|menu| menu.enabled)
                    .and_then(|menu| Operation::from_repr(menu.selected));
                operation.map_or(Action::None, |operation| self.lsp(operation))
            }
            _ => Action::None,
        }
    }

    pub(super) fn lsp(&mut self, operation: Operation) -> Action {
        let Some((line, expected_line)) = self.current_source() else {
            self.toasts
                .push("No current disk source at this row", ToastKind::Info);
            return Action::None;
        };
        let Some(file) = self.selected() else {
            return Action::None;
        };
        let path = file
            .disk_path
            .clone()
            .unwrap_or_else(|| PathBuf::from(&file.path));
        if !crate::is_rust_path(&path) {
            self.toasts.push(
                "Rust navigation is available only for .rs files",
                ToastKind::Info,
            );
            return Action::None;
        }
        let mut byte_column = file.column.min(expected_line.len());
        while !expected_line.is_char_boundary(byte_column) {
            byte_column = byte_column.saturating_sub(1);
        }
        let toast_id = self.toasts.start_long_toast(operation.progress_text());
        Action::Lsp {
            operation,
            query: Query {
                toast_id,
                path,
                line,
                byte_column,
                expected_line,
                snapshot_id: self.commit_id.clone(),
            },
        }
    }

    pub(super) fn current_source(&self) -> Option<(u32, String)> {
        let file = self.selected()?;
        file.diff.source_position(file.cursor)
    }

    pub(super) fn current_word(&self) -> Option<String> {
        let (_, line) = self.current_source()?;
        let column = self.selected()?.column.min(line.len());
        let character = line.get(column..)?.chars().next()?;
        if !is_word_character(character) {
            return None;
        }
        let start = line[..column]
            .char_indices()
            .rev()
            .take_while(|(_, character)| is_word_character(*character))
            .last()
            .map_or(column, |(index, _)| index);
        let end = line[column..]
            .char_indices()
            .take_while(|(_, character)| is_word_character(*character))
            .last()
            .map_or(column, |(index, character)| {
                column + index + character.len_utf8()
            });
        Some(line[start..end].to_owned())
    }

    pub(super) fn set_source_column(&mut self, column: usize) -> Action {
        let Some((_, line)) = self.current_source() else {
            return Action::None;
        };
        let mut column = column.min(line.len());
        while !line.is_char_boundary(column) {
            column = column.saturating_sub(1);
        }
        if let Some(file) = self.files.get_mut(self.selected_file) {
            file.column = column;
            file.clear_source_location();
        }
        self.keep_visible();
        Action::None
    }

    pub(super) fn move_source_column(&mut self, delta: isize) -> Action {
        let Some((_, line)) = self.current_source() else {
            return Action::None;
        };
        let current = self
            .selected()
            .map_or(0, |file| file.column.min(line.len()));
        let target = if delta < 0 {
            line[..current]
                .char_indices()
                .next_back()
                .map_or(0, |(index, _)| index)
        } else {
            line[current..]
                .char_indices()
                .nth(1)
                .map_or(line.len(), |(index, _)| current + index)
        };
        self.set_source_column(target)
    }

    pub(super) fn move_word(&mut self, forward: bool) -> Action {
        let Some((_, line)) = self.current_source() else {
            return Action::None;
        };
        let current = self
            .selected()
            .map_or(0, |file| file.column.min(line.len()));
        let target = if forward {
            let starts_in_word = line[current..]
                .chars()
                .next()
                .is_some_and(is_word_character);
            let mut left_word = !starts_in_word;
            line[current..]
                .char_indices()
                .find_map(|(index, character)| {
                    if is_word_character(character) && left_word {
                        Some(current + index)
                    } else {
                        left_word |= !is_word_character(character);
                        None
                    }
                })
                .unwrap_or(line.len())
        } else {
            let mut target = 0;
            let mut found_word = false;
            for (index, character) in line[..current].char_indices().rev() {
                if is_word_character(character) {
                    target = index;
                    found_word = true;
                } else if found_word {
                    break;
                }
            }
            target
        };
        self.set_source_column(target)
    }
}

fn is_word_character(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

#[cfg(test)]
#[path = "navigation.tests.rs"]
mod tests;
