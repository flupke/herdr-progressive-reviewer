use crate::app::{
    Action, ContextMenu, DiffControl, DragState, Focus, Key, MIN_PANE_WIDTH, PaneLayout,
    PendingReview, ReviewApp, Search, Selection, display_column_to_byte,
};
use crate::presentation::SearchDirection;
use crate::{commit_message, footer};
use ratatui::layout::{Position, Rect};
use review_lsp::Operation;
use review_state::ReviewStatus;
use review_store::OutputTarget;

impl ReviewApp {
    pub(super) fn key(&mut self, key: Key) -> Action {
        if self.context_menu.is_some() {
            return self.context_menu_key(key);
        }
        if self.locations.is_some() {
            return self.location_list_key(key);
        }
        if self.hover.is_some() {
            match key {
                Key::Escape => self.hover = None,
                Key::Down | Key::Char('j') => {
                    self.hover_scroll = self.hover_scroll.saturating_add(1);
                }
                Key::Up | Key::Char('k') => {
                    self.hover_scroll = self.hover_scroll.saturating_sub(1);
                }
                _ => {}
            }
            return Action::None;
        }
        if self.search.as_ref().is_some_and(|search| search.editing) {
            return self.search_key(key);
        }
        if self.awaiting_g_command {
            self.awaiting_g_command = false;
            return match key {
                Key::Char('g') => self.navigate_to(0),
                Key::Char('d') => self.lsp(Operation::Definition),
                Key::Char('r') => self.lsp(Operation::References),
                _ => Action::None,
            };
        }
        self.main_view_key(key)
    }

    fn main_view_key(&mut self, key: Key) -> Action {
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
                self.context_menu = None;
                Action::None
            }
            Key::Enter if self.focus == Focus::Files => self
                .selected()
                .map_or(Action::None, |file| self.output(file.path.clone())),
            Key::Enter => self.insert(),
            Key::Char('o') => self.set_output_target(match self.output_target {
                OutputTarget::ActiveAgent => OutputTarget::Clipboard,
                OutputTarget::Clipboard => OutputTarget::ActiveAgent,
            }),
            Key::Space | Key::Char(' ') => self.toggle_review(),
            Key::Visual | Key::Char('v' | 'V') if self.focus == Focus::Diff => {
                self.visual();
                Action::None
            }
            Key::Char('K') if self.focus == Focus::Diff => self.lsp(Operation::Hover),
            Key::Char('g') => {
                self.awaiting_g_command = true;
                Action::None
            }
            Key::Char('h') if self.focus == Focus::Diff => self.move_source_column(-1),
            Key::Expand | Key::Char('l') if self.focus == Focus::Diff => {
                if self.current_source().is_some() {
                    self.move_source_column(1)
                } else {
                    self.expand_gap();
                    Action::None
                }
            }
            Key::Char('w') if self.focus == Focus::Diff => self.move_word(true),
            Key::Char('b') if self.focus == Focus::Diff => self.move_word(false),
            Key::Char('0') if self.focus == Focus::Diff => self.set_source_column(0),
            Key::Char('$') if self.focus == Focus::Diff => {
                let end = self.current_source().map_or(0, |(_, line)| line.len());
                self.set_source_column(end)
            }
            Key::Down | Key::Char('j') => self.navigate(1),
            Key::Up | Key::Char('k') => self.navigate(-1),
            Key::First => self.navigate_to(0),
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

    pub(super) fn update_search_match(&mut self) {
        let Some(search) = &self.search else {
            return;
        };
        if search.query.is_empty() {
            if let Some(file) = self.files.get_mut(self.selected_file) {
                file.cursor = search.origin.min(file.diff.len().saturating_sub(1));
                file.clear_source_location();
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
            self.select_file(file);
            self.move_diff_cursor(Some(row));
        }
    }

    pub(super) fn finish_pending_search(&mut self) {
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
        let page = self.page_rows();
        if let Some(file) = self.files.get_mut(self.selected_file) {
            file.clear_source_location();
            file.jump_to_row(target, page);
        }
        self.selection = None;
        self.keep_file_visible();
    }

    fn toggle_review(&mut self) -> Action {
        if self.review_in_flight.is_some() {
            return Action::None;
        }
        let Some(file) = self.files.get_mut(self.selected_file) else {
            return Action::None;
        };
        if file.temporary {
            return Action::None;
        }
        let path = file.path.clone();
        let previous_status = file.status;
        let reviewed = previous_status.needs_review();
        let optimistic_status = if reviewed {
            ReviewStatus::Reviewed
        } else {
            ReviewStatus::Unreviewed
        };
        file.status = optimistic_status;
        let next_file = reviewed
            .then(|| {
                self.file_tree
                    .visible_files()
                    .skip_while(|file| *file != self.selected_file)
                    .skip(1)
                    .find(|file| self.files[*file].status.needs_review())
            })
            .flatten();
        let next_path = next_file.map(|file| self.files[file].path.clone());
        if let Some(next_file) = next_file {
            self.select_file(next_file);
            self.selection = None;
            self.keep_visible();
        }
        self.review_in_flight = Some(PendingReview {
            path: path.clone(),
            previous_status,
            optimistic_status,
            next_path,
        });
        Action::SetReviewed { path, reviewed }
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
            Ok(excerpt) => self.output(excerpt.into_string()),
            Err(_) => Action::None,
        }
    }

    fn output(&self, text: String) -> Action {
        Action::Output {
            target: self.output_target,
            text,
        }
    }

    fn set_output_target(&mut self, target: OutputTarget) -> Action {
        if self.output_target == target {
            return Action::None;
        }
        self.output_target = target;
        Action::SaveOutputTarget(target)
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
                    let was_temporary = self
                        .files
                        .get(self.selected_file)
                        .is_some_and(|file| file.temporary);
                    let target_path = self.files[target].path.clone();
                    self.select_file(target);
                    self.selection = None;
                    selected_changed = true;
                    if was_temporary {
                        self.remove_temporary_files();
                        self.selected_file = self
                            .files
                            .iter()
                            .position(|file| file.path == target_path)
                            .unwrap_or(0);
                    }
                }
            }
            Focus::Diff => {
                let Some(file) = self.files.get_mut(self.selected_file) else {
                    return Action::None;
                };
                file.cursor = target.min(file.diff.len().saturating_sub(1));
                file.clear_source_location();
                if let Some(selection) = &mut self.selection
                    && !selection.fixed
                {
                    selection.cursor = file.cursor;
                }
            }
        }
        self.keep_visible();
        if self.focus == Focus::Diff {
            let column = self.selected().map_or(0, |file| file.column);
            let _ = self.set_source_column(column);
        }
        if selected_changed {
            self.load_selected_action()
        } else {
            Action::None
        }
    }

    pub(super) fn mouse_scroll(&mut self, column: u16, row: u16, delta: isize) -> Action {
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
        if self.locations.is_some() {
            return self.move_location(delta);
        }
        let focused = self.focus;
        self.focus = hovered;
        let action = self.navigate(delta);
        self.focus = focused;
        action
    }

    pub(super) fn mouse_click(&mut self, column: u16, row: u16, insert_path: bool) -> Action {
        if let Some(menu) = self.context_menu.take() {
            return self.context_menu_click(&menu, column, row);
        }
        if self.locations.is_some()
            && self.layout().focus_at(self.focus, column, row) == Some(Focus::Files)
            && self.layout().contains_pane_content(row)
        {
            let scroll = self.locations.as_ref().map_or(0, |list| list.scroll);
            return self.move_location_to(scroll + usize::from(row - 2));
        }
        if self.show_commit_message {
            let area =
                commit_message::CommitMessageView::area(Rect::new(0, 0, self.width, self.height));
            if !area.contains(Position::new(column, row)) {
                self.show_commit_message = false;
            }
            return Action::None;
        }
        let layout = self.layout();
        if self.commit_title_at(column, row) {
            self.show_commit_message = !self.show_commit_message;
            return Action::None;
        }
        if row == self.height.saturating_sub(2)
            && let Some(target) = footer::FooterView::output_target_at(column)
        {
            return self.set_output_target(target);
        }
        if let Some(control) = layout.diff_control_at(self.focus, column, row, self.selected()) {
            return self.diff_control_click(control);
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
            self.position_diff_cursor(layout, column, row);
            let Some(file) = self.files.get_mut(self.selected_file) else {
                return Action::None;
            };
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
        if focus != Focus::Files {
            return Action::None;
        }
        self.file_click(layout, column, row, insert_path)
    }

    fn file_click(
        &mut self,
        layout: PaneLayout,
        column: u16,
        row: u16,
        insert_path: bool,
    ) -> Action {
        if !layout.contains_pane_content(row) {
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
        let was_temporary = self
            .files
            .get(self.selected_file)
            .is_some_and(|file| file.temporary);
        let target_path = self.files[target].path.clone();
        self.select_file(target);
        self.selection = None;
        if was_temporary && !self.files[target].temporary {
            self.remove_temporary_files();
            self.selected_file = self
                .files
                .iter()
                .position(|file| file.path == target_path)
                .unwrap_or(0);
        }
        if insert_path {
            return self.output(self.files[target].path.clone());
        }
        self.load_selected_action()
    }

    fn context_menu_click(&mut self, menu: &ContextMenu, column: u16, row: u16) -> Action {
        let area = menu.area(Rect::new(0, 0, self.width, self.height));
        let item = row.checked_sub(area.y.saturating_add(1)).map(usize::from);
        if area.contains(Position::new(column, row))
            && menu.enabled
            && let Some(operation) = item.and_then(Operation::from_repr)
        {
            return self.lsp(operation);
        }
        Action::None
    }

    fn diff_control_click(&mut self, control: DiffControl) -> Action {
        self.focus = Focus::Diff;
        let Some(file) = self.files.get_mut(self.selected_file) else {
            return Action::None;
        };
        let changed = match control {
            DiffControl::ExpandAll => file.diff.expand_all(),
            DiffControl::ContractAll => file.diff.contract_all(),
            DiffControl::ShowFile => file.diff.show_file(),
            DiffControl::CloseFile => file.diff.show_diff(),
        };
        if changed {
            self.selection = None;
            file.cursor = 0;
            file.scroll = 0;
            file.clear_source_location();
            self.keep_visible();
        }
        Action::None
    }

    fn position_diff_cursor(&mut self, layout: PaneLayout, column: u16, row: u16) {
        let page_row = usize::from(row - 2);
        let code_column = layout.source_column(column, self.selected());
        let Some(file) = self.files.get_mut(self.selected_file) else {
            return;
        };
        file.cursor = (file.scroll + page_row).min(file.diff.len().saturating_sub(1));
        file.clear_source_location();
        if let Some(display_column) = code_column
            && let Some((_, line)) = file.diff.source_position(file.cursor)
        {
            file.column = display_column_to_byte(&line, display_column);
        }
    }

    pub(super) fn mouse_control_click(&mut self, column: u16, row: u16) -> Action {
        if self.hovered_focus(column, row) == Some(Focus::Diff) {
            self.mouse_right_click(column, row)
        } else {
            self.mouse_click(column, row, true)
        }
    }

    pub(super) fn mouse_right_click(&mut self, column: u16, row: u16) -> Action {
        let layout = self.layout();
        if layout.focus_at(self.focus, column, row) != Some(Focus::Diff)
            || !layout.contains_pane_content(row)
        {
            self.context_menu = None;
            return Action::None;
        }
        self.focus = Focus::Diff;
        self.position_diff_cursor(layout, column, row);
        let enabled = self.current_source().is_some();
        self.context_menu = Some(ContextMenu {
            column,
            row,
            selected: 0,
            enabled,
        });
        Action::None
    }

    pub(super) fn mouse_double_click(&mut self, column: u16, row: u16) -> Action {
        let layout = self.layout();
        if self.locations.is_some()
            && layout.focus_at(self.focus, column, row) == Some(Focus::Files)
            && layout.contains_pane_content(row)
        {
            let scroll = self.locations.as_ref().map_or(0, |list| list.scroll);
            let _ = self.move_location_to(scroll + usize::from(row - 2));
            return self
                .locations
                .as_ref()
                .and_then(|list| list.locations.get(list.selected))
                .cloned()
                .map_or(Action::None, |location| self.accept_location(location));
        }
        if layout.focus_at(self.focus, column, row) != Some(Focus::Files)
            || !layout.contains_pane_content(row)
        {
            return Action::None;
        }
        let row = self.file_scroll + usize::from(row - 2);
        if self.file_tree.file_at(row) != Some(self.selected_file) {
            return Action::None;
        }
        self.toggle_review()
    }

    pub(super) fn mouse_drag(&mut self, column: u16, row: u16) -> Action {
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
                file.clear_source_location();
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

    pub(super) fn mouse_release(&mut self) -> Action {
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
}
