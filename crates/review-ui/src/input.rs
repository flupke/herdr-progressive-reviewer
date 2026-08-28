use crate::app::{
    Action, ActivePopup, ContextMenu, DiffControl, DragState, Focus, Key, MIN_PANE_WIDTH,
    PaneLayout, PendingGuideJump, PendingReview, ReviewApp, Search, Selection,
    display_column_to_byte,
};
use crate::diff::{DiffView, GuideTargetPosition};
use crate::presentation::SearchDirection;
use crate::shortcuts::{
    self, ApplicationShortcut, GuideShortcut, LspShortcut, NavigationShortcut, SearchShortcut,
    ShortcutCommand, ShortcutLookup, SourceShortcut,
};
use crate::{commit_message, footer, help};
use ratatui::layout::{Position, Rect};
use review_guide::GuideTarget;
use review_lsp::Operation;
use review_state::ReviewStatus;
use review_store::OutputTarget;
use toasts::ToastKind;

#[derive(Clone, Debug)]
struct GuideCommentTarget {
    file_index: usize,
    target: GuideTarget,
    row: Option<usize>,
}

impl ReviewApp {
    pub(super) fn key(&mut self, key: Key) -> Action {
        if self.active_popup == Some(ActivePopup::RevisionSelector) {
            return self.revision_selector_key(key);
        }
        if self.active_popup == Some(ActivePopup::ShortcutHelp) {
            return self.shortcut_help_key(key);
        }
        if self.context_menu.is_some() {
            return self.context_menu_key(key);
        }
        if self.locations.is_some() {
            return self.location_list_key(key);
        }
        if self.hover.is_some() {
            return self.hover_key(key);
        }
        if self.search.as_ref().is_some_and(|search| search.editing) {
            return self.search_key(key);
        }
        self.main_view_key(key)
    }

    fn shortcut_help_key(&mut self, key: Key) -> Action {
        if shortcuts::closes_help(key) {
            self.active_popup = None;
        }
        if shortcuts::lookup(None, key)
            == ShortcutLookup::Command(ShortcutCommand::Application(ApplicationShortcut::Clear))
        {
            self.selection = None;
            self.search = None;
            self.context_menu = None;
        }
        match shortcuts::lookup(None, key) {
            ShortcutLookup::Command(ShortcutCommand::Navigation(NavigationShortcut::MoveDown)) => {
                self.shortcut_help_scroll = self
                    .shortcut_help_scroll
                    .saturating_add(1)
                    .min(help::ShortcutHelpView::maximum_scroll(self.height));
            }
            ShortcutLookup::Command(ShortcutCommand::Navigation(NavigationShortcut::MoveUp)) => {
                self.shortcut_help_scroll = self.shortcut_help_scroll.saturating_sub(1);
            }
            _ => {}
        }
        Action::None
    }

    fn hover_key(&mut self, key: Key) -> Action {
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
        Action::None
    }

    fn generate_review_guide(&mut self, scope: review_guide::GuideScope) -> Action {
        let action = Action::GenerateReviewGuide { scope };
        if self.guide_spinner_frame.is_some() {
            self.toasts
                .push("A guide update is already in progress", ToastKind::Info);
            return Action::None;
        }
        action
    }

    fn main_view_key(&mut self, key: Key) -> Action {
        let prefix = self.pending_shortcut_prefix.take();
        match shortcuts::lookup(prefix, key) {
            ShortcutLookup::Command(command) => self.run_shortcut(command),
            ShortcutLookup::Prefix(prefix) => {
                self.pending_shortcut_prefix = Some(prefix);
                Action::None
            }
            ShortcutLookup::None => Action::None,
        }
    }

    fn run_shortcut(&mut self, command: ShortcutCommand) -> Action {
        match command {
            ShortcutCommand::Application(command) => self.run_application_shortcut(command),
            ShortcutCommand::Guide(command) => self.run_guide_shortcut(command),
            ShortcutCommand::Lsp(command) => self.run_lsp_shortcut(command),
            ShortcutCommand::Navigation(command) => self.run_navigation_shortcut(command),
            ShortcutCommand::Search(command) => self.run_search_shortcut(command),
            ShortcutCommand::Source(command) => self.run_source_shortcut(command),
        }
    }

    fn run_search_shortcut(&mut self, command: SearchShortcut) -> Action {
        match command {
            SearchShortcut::Begin => self.start_search(),
            SearchShortcut::NextMatch => {
                self.repeat_search(SearchDirection::Forward);
                Action::None
            }
            SearchShortcut::PreviousMatch => {
                self.repeat_search(SearchDirection::Backward);
                Action::None
            }
        }
    }

    fn run_application_shortcut(&mut self, command: ApplicationShortcut) -> Action {
        match command {
            ApplicationShortcut::Quit => Action::Quit,
            ApplicationShortcut::ShowCommitMessage => {
                self.active_popup = (self.active_popup != Some(ActivePopup::CommitMessage))
                    .then_some(ActivePopup::CommitMessage);
                Action::None
            }
            ApplicationShortcut::OpenHelp => {
                self.active_popup = Some(ActivePopup::ShortcutHelp);
                self.shortcut_help_scroll = 0;
                Action::None
            }
            ApplicationShortcut::ChangeFocus => {
                self.focus = match self.focus {
                    Focus::Files => Focus::Diff,
                    Focus::Diff => Focus::Files,
                };
                Action::None
            }
            ApplicationShortcut::Clear => {
                self.active_popup = None;
                self.selection = None;
                self.search = None;
                self.context_menu = None;
                Action::None
            }
            ApplicationShortcut::Insert if self.focus == Focus::Files => self
                .selected()
                .map_or(Action::None, |file| self.output(file.path.clone())),
            ApplicationShortcut::Insert => self.insert(),
            ApplicationShortcut::ChangeOutputTarget => {
                self.set_output_target(match self.output_target {
                    OutputTarget::ActiveAgent => OutputTarget::Clipboard,
                    OutputTarget::Clipboard => OutputTarget::ActiveAgent,
                })
            }
            ApplicationShortcut::MarkReviewed => self.toggle_review(),
            ApplicationShortcut::StartSelection if self.focus == Focus::Diff => {
                self.visual();
                Action::None
            }
            ApplicationShortcut::StartSelection => Action::None,
        }
    }

    fn run_guide_shortcut(&mut self, command: GuideShortcut) -> Action {
        match command {
            GuideShortcut::GenerateSelectedFile => {
                let Some(path) = self.selected().map(|file| file.path.clone()) else {
                    return Action::None;
                };
                self.generate_review_guide(review_guide::GuideScope::File { path })
            }
            GuideShortcut::GenerateFull => {
                self.generate_review_guide(review_guide::GuideScope::All)
            }
            GuideShortcut::GoToPreviousComment => {
                self.jump_to_guide_comment(SearchDirection::Backward)
            }
            GuideShortcut::GoToNextComment => self.jump_to_guide_comment(SearchDirection::Forward),
        }
    }

    fn run_lsp_shortcut(&mut self, command: LspShortcut) -> Action {
        match command {
            LspShortcut::ShowDocumentation if self.focus == Focus::Diff => {
                self.lsp(Operation::Hover)
            }
            LspShortcut::GoToDefinition => self.lsp(Operation::Definition),
            LspShortcut::GoToReferences => self.lsp(Operation::References),
            LspShortcut::Restart => Action::RestartLsp,
            LspShortcut::ShowDocumentation => Action::None,
        }
    }

    fn run_navigation_shortcut(&mut self, command: NavigationShortcut) -> Action {
        match command {
            NavigationShortcut::MoveDown => self.navigate(1),
            NavigationShortcut::MoveUp => self.navigate(-1),
            NavigationShortcut::GoToFirst => self.jump_to(0),
            NavigationShortcut::GoToLast => {
                let last = self.focus_len().saturating_sub(1);
                self.jump_to(last)
            }
            NavigationShortcut::MoveHalfPageDown => self.navigate_half_page(self.half_page_rows()),
            NavigationShortcut::MoveHalfPageUp => self.navigate_half_page(-self.half_page_rows()),
            NavigationShortcut::GoToPreviousLocation => self.previous_location(),
            NavigationShortcut::GoToNextLocation => self.next_location(),
            NavigationShortcut::GoToParentRevision => self.start_revision_navigation(
                review_repository::repository::RevisionDirection::Parents,
            ),
            NavigationShortcut::GoToChildRevision => self.start_revision_navigation(
                review_repository::repository::RevisionDirection::Children,
            ),
        }
    }

    fn start_search(&mut self) -> Action {
        self.focus = Focus::Diff;
        self.selection = None;
        let origin_location = self.current_review_location();
        self.search = Some(Search {
            query: String::new(),
            origin: self.selected().map_or(0, |file| file.cursor),
            origin_location,
            editing: true,
            pending: Vec::new(),
        });
        self.load_all_diffs_action()
    }

    fn run_source_shortcut(&mut self, command: SourceShortcut) -> Action {
        if self.focus != Focus::Diff {
            return Action::None;
        }
        match command {
            SourceShortcut::MoveLeft => self.move_source_column(-1),
            SourceShortcut::ExpandOrMoveRight => {
                if self.current_source().is_some() {
                    self.move_source_column(1)
                } else {
                    self.expand_gap();
                    Action::None
                }
            }
            SourceShortcut::MoveToNextWord => self.move_word(true),
            SourceShortcut::MoveToPreviousWord => self.move_word(false),
            SourceShortcut::MoveToStartOfLine => self.set_source_column(0),
            SourceShortcut::MoveToEndOfLine => {
                let end = self.current_source().map_or(0, |(_, line)| line.len());
                self.set_source_column(end)
            }
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
                let origin = self
                    .search
                    .as_ref()
                    .and_then(|search| search.origin_location.clone());
                if let Some(search) = &mut self.search {
                    search.editing = false;
                }
                if self.record_location_change(origin) {
                    self.center_selected_location();
                }
            }
            Key::Escape => {
                let origin = self.search.as_ref().map(|search| search.origin);
                self.search = None;
                if let (Some(origin), Some(file)) = (origin, self.files.get_mut(self.selected_file))
                {
                    file.cursor = origin.min(file.diff.len().saturating_sub(1));
                    file.clear_source_location();
                }
                self.keep_visible();
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
            let _ = self.jump(|app| {
                app.select_file(file);
                app.move_diff_cursor(Some(row));
                Action::None
            });
        }
    }

    fn jump_to_guide_comment(&mut self, direction: SearchDirection) -> Action {
        let targets = self.guide_comment_targets();
        let current = (
            self.selected_file,
            self.selected().map_or(0, |file| file.cursor),
        );
        let target = match direction {
            SearchDirection::Forward => targets
                .iter()
                .find(|target| {
                    target.file_index > current.0
                        || (target.file_index == current.0
                            && target.row.is_some_and(|row| row > current.1))
                })
                .or_else(|| targets.first()),
            SearchDirection::Backward => targets
                .iter()
                .rev()
                .find(|target| {
                    target.file_index < current.0
                        || (target.file_index == current.0
                            && target.row.is_some_and(|row| row < current.1))
                })
                .or_else(|| targets.last()),
        }
        .cloned();
        target.map_or(Action::None, |target| self.jump_to_guide_target(target))
    }

    fn guide_comment_targets(&self) -> Vec<GuideCommentTarget> {
        let mut targets = Vec::new();
        for (file_index, file) in self.files.iter().enumerate() {
            if !file.status.needs_review() {
                continue;
            }
            let start = targets.len();
            targets.extend(
                self.guide_items
                    .iter()
                    .filter(|item| item.target.path() == file.path)
                    .filter_map(|item| {
                        let row = match DiffView::guide_target_position(file, &item.target)? {
                            GuideTargetPosition::Unloaded => None,
                            GuideTargetPosition::Rows { first } => Some(first),
                        };
                        Some(GuideCommentTarget {
                            file_index,
                            row,
                            target: item.target.clone(),
                        })
                    }),
            );
            targets[start..].sort_by_key(|target| target.row.unwrap_or(usize::MAX));
        }
        targets
    }

    fn jump_to_guide_target(&mut self, target: GuideCommentTarget) -> Action {
        if let Some(row) = target.row {
            let action = self.jump(|app| {
                app.select_file(target.file_index);
                app.focus = Focus::Diff;
                app.move_diff_cursor(Some(row));
                Action::None
            });
            self.position_guide_at_top(row);
            return action;
        }
        let origin = self.current_review_location();
        self.select_file(target.file_index);
        self.focus = Focus::Diff;
        self.selection = None;
        self.pending_guide_jump = Some(PendingGuideJump {
            target: target.target,
            origin,
        });
        let action = self.load_selected_action();
        if action == Action::None {
            self.pending_guide_jump = None;
            self.toasts
                .push("Guide comment is not available", ToastKind::Info);
        }
        action
    }

    pub(super) fn finish_pending_guide_jump(&mut self, path: &str) {
        let Some(pending) = self
            .pending_guide_jump
            .take_if(|pending| pending.target.path() == path)
        else {
            return;
        };
        let target = self.selected().and_then(|file| {
            (file.path == path)
                .then(|| DiffView::guide_target_rows(file, &pending.target))
                .flatten()
                .map(|(first, _)| first)
        });
        let Some(target) = target else {
            self.toasts
                .push("Guide comment is not available", ToastKind::Info);
            return;
        };
        self.move_diff_cursor(Some(target));
        self.record_location_change(pending.origin);
        self.position_guide_at_top(target);
    }

    fn position_guide_at_top(&mut self, target: usize) {
        let layout = self.layout();
        let scroll = self.selected().and_then(|file| {
            DiffView(self)
                .viewport(file, layout.diff_content_width(), false)
                .guide_start_visual_row(target)
        });
        if let (Some(scroll), Some(file)) = (scroll, self.files.get_mut(self.selected_file)) {
            file.scroll = scroll;
        }
        self.keep_file_visible();
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
        self.keep_visible();
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
        self.rebuild_guide_item_counters();
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
            let _ = self.jump(|app| {
                app.select_file(next_file);
                app.selection = None;
                app.keep_visible();
                Action::None
            });
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
        let target = current.saturating_add_signed(delta);
        if self.focus == Focus::Files {
            self.jump(|app| app.navigate_to(target))
        } else {
            self.navigate_to(target)
        }
    }

    fn navigate_half_page(&mut self, delta: isize) -> Action {
        if self.focus != Focus::Diff {
            return self.navigate(delta);
        }
        let layout = self.layout();
        let target = self.selected().and_then(|file| {
            DiffView(self)
                .viewport(file, layout.diff_content_width(), false)
                .source_position_after_visual_delta(file, delta)
        });
        let Some((source_row, source_column)) = target else {
            return Action::None;
        };
        let Some(file) = self.files.get_mut(self.selected_file) else {
            return Action::None;
        };
        file.cursor = source_row;
        file.clear_source_location();
        if let Some((_, line)) = file.diff.source_position(source_row) {
            file.column = display_column_to_byte(&line, source_column);
        }
        if let Some(selection) = &mut self.selection
            && !selection.fixed
        {
            selection.cursor = source_row;
        }
        self.keep_visible();
        Action::None
    }

    fn jump_to(&mut self, target: usize) -> Action {
        self.jump(|app| app.navigate_to(target))
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
            let layout = self.layout();
            let page = layout.page_rows();
            let scroll = self.displayed().map(|file| {
                let viewport = DiffView(self).viewport(file, layout.diff_content_width(), false);
                viewport
                    .scroll(file)
                    .saturating_add_signed(delta)
                    .min(viewport.len().saturating_sub(page))
            });
            if let Some(file) = self.displayed_mut() {
                file.scroll = scroll.unwrap_or(0);
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
        if let Some(action) = self.overlay_mouse_click(column, row) {
            return action;
        }
        let layout = self.layout();
        if let Some(action) = self.chrome_mouse_click(layout, column, row) {
            return action;
        }
        self.pane_mouse_click(layout, column, row, insert_path)
    }

    fn overlay_mouse_click(&mut self, column: u16, row: u16) -> Option<Action> {
        if self.active_popup == Some(ActivePopup::ShortcutHelp) {
            let area = help::ShortcutHelpView::area(Rect::new(0, 0, self.width, self.height));
            if !area.contains(Position::new(column, row)) {
                self.active_popup = None;
            }
            return Some(Action::None);
        }
        if let Some(menu) = self.context_menu.take() {
            return Some(self.context_menu_click(&menu, column, row));
        }
        if self.locations.is_some()
            && self.layout().focus_at(self.focus, column, row) == Some(Focus::Files)
            && self.layout().contains_pane_content(row)
        {
            let scroll = self.locations.as_ref().map_or(0, |list| list.scroll);
            return Some(self.move_location_to(scroll + usize::from(row - 2)));
        }
        if self.active_popup == Some(ActivePopup::CommitMessage) {
            let area =
                commit_message::CommitMessageView::area(Rect::new(0, 0, self.width, self.height));
            if !area.contains(Position::new(column, row)) {
                self.active_popup = None;
            }
            return Some(Action::None);
        }
        None
    }

    fn chrome_mouse_click(&mut self, layout: PaneLayout, column: u16, row: u16) -> Option<Action> {
        if self.commit_title_at(column, row) {
            self.active_popup = (self.active_popup != Some(ActivePopup::CommitMessage))
                .then_some(ActivePopup::CommitMessage);
            return Some(Action::None);
        }
        if row == self.height.saturating_sub(1)
            && let Some(target) = footer::FooterView::output_target_at(column)
        {
            return Some(self.set_output_target(target));
        }
        if self.preview.is_some()
            && !layout.is_separator(column, row)
            && layout.focus_at(self.focus, column, row) == Some(Focus::Diff)
        {
            return Some(Action::None);
        }
        if let Some(control) = layout.diff_control_at(self.focus, column, row, self.selected()) {
            return Some(self.diff_control_click(control));
        }
        self.drag = if layout.is_separator(column, row) {
            DragState::Resize { moved: false }
        } else {
            DragState::None
        };
        if matches!(self.drag, DragState::Resize { .. }) {
            return Some(Action::None);
        }
        None
    }

    fn pane_mouse_click(
        &mut self,
        layout: PaneLayout,
        column: u16,
        row: u16,
        insert_path: bool,
    ) -> Action {
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
        self.jump(|app| app.file_click_without_history(layout, column, row, insert_path))
    }

    fn file_click_without_history(
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
            return self.output(target_path);
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
        let Some((source_row, source_column)) = self.diff_position(layout, column, row) else {
            return;
        };
        let Some(file) = self.files.get_mut(self.selected_file) else {
            return;
        };
        file.cursor = source_row;
        file.clear_source_location();
        if let Some(display_column) = source_column
            && let Some((_, line)) = file.diff.source_position(file.cursor)
        {
            file.column = display_column_to_byte(&line, display_column);
        }
    }

    fn diff_position(
        &self,
        layout: PaneLayout,
        column: u16,
        row: u16,
    ) -> Option<(usize, Option<usize>)> {
        let file = self.selected()?;
        let viewport = DiffView(self).viewport(file, layout.diff_content_width(), false);
        let visual_row = viewport
            .scroll(file)
            .saturating_add(usize::from(row.saturating_sub(2)));
        let source_row = viewport.source_row_at(visual_row)?;
        let pane_column = usize::from(column.saturating_sub(layout.diff_content_start_column()));
        let source_column =
            viewport.source_column_at(visual_row, pane_column, file.diff.line_number_width());
        Some((source_row, source_column))
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
        if self.preview.is_some()
            || layout.focus_at(self.focus, column, row) != Some(Focus::Diff)
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
        if self.preview.is_some() && matches!(self.drag, DragState::Select { .. }) {
            self.drag = DragState::None;
            return Action::None;
        }
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
                let Some((source_row, _)) = self.diff_position(layout, column, row) else {
                    return Action::None;
                };
                let Some(file) = self.files.get_mut(self.selected_file) else {
                    return Action::None;
                };
                file.cursor = source_row;
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

#[cfg(test)]
#[path = "input.tests.rs"]
mod tests;
