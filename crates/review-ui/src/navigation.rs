use std::path::PathBuf;

use crate::app::{Action, Focus, Key, LocationList, ReviewApp, ReviewFile, SourceLoadMode};
use crate::presentation::DiffPresentation;
use review_lsp::{Operation, Query, SourceLocation};
use toasts::ToastKind;
use unicode_width::UnicodeWidthStr;

impl ReviewApp {
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
            self.keep_file_visible();
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

    pub(super) fn set_source_column(&mut self, column: usize) -> Action {
        let Some((_, line)) = self.current_source() else {
            return Action::None;
        };
        let mut column = column.min(line.len());
        while !line.is_char_boundary(column) {
            column = column.saturating_sub(1);
        }
        let pane_width = usize::from(self.layout().diff_width());
        let number_width = self
            .selected()
            .map_or(0, |file| file.diff.line_number_width());
        if let Some(file) = self.files.get_mut(self.selected_file) {
            file.column = column;
            file.clear_source_location();
            let line_width = line[..column].width();
            let code_width = pane_width.saturating_sub(number_width + 5).max(1);
            if line_width < file.horizontal_scroll {
                file.horizontal_scroll = line_width;
            } else if line_width >= file.horizontal_scroll + code_width {
                file.horizontal_scroll = line_width + 1 - code_width;
            }
        }
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
        let word = |character: char| character.is_alphanumeric() || character == '_';
        let target = if forward {
            let starts_in_word = line[current..].chars().next().is_some_and(word);
            let mut left_word = !starts_in_word;
            line[current..]
                .char_indices()
                .find_map(|(index, character)| {
                    if word(character) && left_word {
                        Some(current + index)
                    } else {
                        left_word |= !word(character);
                        None
                    }
                })
                .unwrap_or(line.len())
        } else {
            let mut target = 0;
            let mut found_word = false;
            for (index, character) in line[..current].char_indices().rev() {
                if word(character) {
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
