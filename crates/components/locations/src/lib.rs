//! Language-server location selection.

use std::path::PathBuf;

use component_core::{AnyInput, Component, ComponentSubscriptions, EventPublisher, InputScope};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use review_lsp::{Event as LspEvent, Operation, SourceLocation};
use ui_actions::Action;
use ui_events::{
    FilesViewportChanged, LocationListVisibilityChanged, LocationSelection, PointerInput,
    PointerInputKind, RepositoryMetadataChanged, SourceLocationAccepted,
    SourceLocationPreviewRequested, ToastRequested,
};
use ui_panes::SelectionPane;
use ui_shortcuts::Key;
use ui_theme::Palette;

/// LSP result-list state, input, and rendering.
pub struct LocationsComponent {
    events: EventPublisher,
    repository_root: PathBuf,
    snapshot_id: String,
    source_session: Option<String>,
    page_rows: usize,
    palette: Palette,
    list: Option<LocationSelection>,
}

impl LocationsComponent {
    pub fn new(events: EventPublisher, repository_root: PathBuf, palette: Palette) -> Self {
        Self {
            events,
            repository_root,
            snapshot_id: String::new(),
            source_session: None,
            page_rows: 1,
            palette,
            list: None,
        }
    }

    pub fn is_active(&self) -> bool {
        self.list.is_some()
    }

    pub fn render(&self, area: Rect, buffer: &mut Buffer, focused: bool) {
        let Some(list) = &self.list else {
            return;
        };
        let border_color = if focused {
            self.palette.focus
        } else {
            self.palette.dim
        };
        SelectionPane::new(area, list.scroll).render(
            buffer,
            format!(
                " {}{} ",
                list.operation.title(),
                if focused { " (focus)" } else { "" }
            ),
            Style::default().fg(border_color),
            &list.locations,
            |index, location, width| {
                let text = format!(
                    "{}:{}:{}",
                    location.display_path(&self.repository_root),
                    location.line.saturating_add(1),
                    location.byte_column.saturating_add(1)
                );
                let background = if index == list.selected {
                    self.palette.cursor
                } else {
                    Color::default()
                };
                let style = Style::default()
                    .fg(self.palette.focus)
                    .add_modifier(Modifier::BOLD)
                    .bg(background);
                Line::styled(ui_panes::shorten(&text, usize::from(width)), style)
            },
        );
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn viewport_changed(&mut self, event: &FilesViewportChanged) {
        self.page_rows = event.rows.max(1);
        self.keep_selected_visible();
    }

    fn repository_changed(&mut self, event: &RepositoryMetadataChanged) {
        self.snapshot_id
            .clone_from(&event.review_checkpoint.checkpoint);
        self.close();
    }

    fn source_session_changed(&mut self, event: &ui_events::SourceSessionChanged) {
        self.source_session.clone_from(&event.snapshot_id);
        self.close();
    }

    fn active_snapshot(&self) -> &str {
        self.source_session.as_deref().unwrap_or(&self.snapshot_id)
    }

    fn lsp_event(&mut self, event: &LspEvent) {
        let LspEvent::Locations {
            operation,
            snapshot_id,
            locations,
            ..
        } = event
        else {
            return;
        };
        if snapshot_id != self.active_snapshot() {
            return;
        }
        match locations.as_slice() {
            [] => self.events.publish(ToastRequested {
                text: "No locations found".to_owned(),
                kind: toasts::ToastKind::Info,
            }),
            [location]
                if matches!(operation, Operation::Definition | Operation::TypeDefinition) =>
            {
                self.events.publish(SourceLocationAccepted {
                    location: location.clone(),
                });
            }
            _ => {
                self.list = Some(LocationSelection {
                    operation: *operation,
                    locations: locations.clone(),
                    selected: 0,
                    scroll: 0,
                });
                self.events
                    .publish(LocationListVisibilityChanged { visible: true });
                self.publish_preview();
            }
        }
    }

    fn keyboard_input(&mut self, key: Key) {
        if self.list.is_none() {
            return;
        }
        match key {
            Key::Escape => self.close(),
            Key::Enter => {
                if let Some(location) = self.selected_location() {
                    self.events.publish(SourceLocationAccepted { location });
                }
                self.close();
            }
            Key::Down | Key::Char('j') => self.move_selection(1),
            Key::Up | Key::Char('k') => self.move_selection(-1),
            Key::First | Key::Char('g') => self.move_selection_to(0),
            Key::Last | Key::Char('G') => {
                let last = self
                    .list
                    .as_ref()
                    .map_or(0, |list| list.locations.len().saturating_sub(1));
                self.move_selection_to(last);
            }
            _ => {}
        }
    }

    fn pointer_input(&mut self, input: PointerInput) {
        let Some(list) = self.list.as_ref() else {
            return;
        };
        match input.kind {
            PointerInputKind::Click | PointerInputKind::DoubleClick => {
                let Some(position) = input.position else {
                    return;
                };
                let selected =
                    SelectionPane::index_for_component_row(list.scroll, position.component_row);
                self.move_selection_to(selected);
                if input.kind == PointerInputKind::DoubleClick {
                    if let Some(location) = self.selected_location() {
                        self.events.publish(SourceLocationAccepted { location });
                    }
                    self.close();
                }
            }
            PointerInputKind::Scroll(delta) => self.move_selection(delta),
            PointerInputKind::ControlClick
            | PointerInputKind::RightClick
            | PointerInputKind::Drag
            | PointerInputKind::Release => {}
        }
    }

    fn move_selection(&mut self, delta: isize) {
        let selected = self.list.as_ref().map_or(0, |list| list.selected);
        self.move_selection_to(selected.saturating_add_signed(delta));
    }

    fn move_selection_to(&mut self, selected: usize) {
        let Some(list) = &mut self.list else {
            return;
        };
        list.selected = selected.min(list.locations.len().saturating_sub(1));
        self.keep_selected_visible();
        self.publish_preview();
    }

    fn keep_selected_visible(&mut self) {
        let Some(list) = &mut self.list else {
            return;
        };
        if list.selected < list.scroll {
            list.scroll = list.selected;
        } else if list.selected >= list.scroll.saturating_add(self.page_rows) {
            list.scroll = list
                .selected
                .saturating_add(1)
                .saturating_sub(self.page_rows);
        }
    }

    fn selected_location(&self) -> Option<SourceLocation> {
        let list = self.list.as_ref()?;
        list.locations.get(list.selected).cloned()
    }

    fn publish_preview(&self) {
        if let Some(location) = self.selected_location() {
            self.events
                .publish(SourceLocationPreviewRequested { location });
        }
    }

    fn close(&mut self) {
        if self.list.take().is_some() {
            self.events
                .publish(LocationListVisibilityChanged { visible: false });
        }
    }
}

impl Component<Action> for LocationsComponent {
    fn register_subscriptions(subscriptions: &mut ComponentSubscriptions<'_, Self, Action>) {
        subscriptions.subscribe(Self::repository_changed);
        subscriptions.subscribe(Self::source_session_changed);
        subscriptions.subscribe(Self::lsp_event);
        subscriptions.subscribe(Self::viewport_changed);
        subscriptions.subscribe_input(InputScope::Focused, AnyInput, Self::keyboard_input);
        subscriptions.subscribe_input(InputScope::Hovered, AnyInput, Self::pointer_input);
    }
}

#[cfg(test)]
#[path = "lib.tests.rs"]
mod tests;
