//! Application coordination for mounted review UI components.

use std::path::PathBuf;

use comment_editor::{EditorKeymap, KeymapSetting};
use component_core::{
    ApplicationEvent, ComponentEventBus, ComponentTarget, DispatchError, DispatchResult,
    EventEnvelope, InputResolution, IntoDispatchResult,
};
use diff_component::{DiffComponent, SyntaxHighlighter};
use explore_component::ExploreComponent;
use files_component::FilesComponent;
use locations_component::LocationsComponent;
use overlay_component::OverlayComponent;
use ratatui::layout::Rect;
use revision_component::RevisionComponent;
use status_component::StatusComponent;
use threads_component::ThreadsComponent;
use ui_events::{
    DiffInputClearRequested, DiffViewportChanged, FilesViewportChanged, PointerInput,
    PointerInputKind, PointerPosition, ReviewNavigation, ReviewNavigationChanged, ReviewPane,
    ReviewableFiles, ViewportChanged,
};

use crate::layout::{Body, NavigationTabs, PaneLayout, ScreenLayout, Viewport};
use crate::navigation::{Modal, Navigation, NavigationRequests, OpenModals, PaneComponent};
use crate::{Action, ApplicationFrame, SettingsAction, TerminalAction, Theme, UserInput};
use ui_theme::Palette;

/// The root coordinator for the review UI.
pub struct ReviewApplication {
    event_bus: ComponentEventBus<Action>,
    hovered_component: Option<ComponentTarget>,
    files_component: ComponentTarget,
    threads_component: ComponentTarget,
    explore_component: ComponentTarget,
    diff_component: ComponentTarget,
    locations_component: ComponentTarget,
    status_component: ComponentTarget,
    overlay_component: ComponentTarget,
    revision_component: ComponentTarget,
    navigation_requests: ComponentTarget,
    viewport: Viewport,
    navigation: Navigation,
    palette: Palette,
    component_areas: Vec<ComponentArea>,
    global_input_pending: bool,
    application_shortcuts: ui_shortcuts::ShortcutMatcher<ui_shortcuts::ApplicationCommand>,
    file_pane_resize: Option<FilePaneResize>,
    editor_keymap: KeymapSetting,
    saved_editor_keymap: EditorKeymap,
    #[cfg(test)]
    focused_for_test: Option<ComponentTarget>,
}

#[derive(Clone, Copy)]
struct ComponentArea {
    target: ComponentTarget,
    area: Rect,
}

#[derive(Clone, Copy)]
struct FilePaneResize {
    moved: bool,
}

impl Default for ReviewApplication {
    fn default() -> Self {
        Self::new(Theme::default(), None, PathBuf::new())
    }
}

impl ReviewApplication {
    /// Run ticks for deferred file loads, animation, and toast deadlines.
    pub fn needs_tick(&self, previous: std::time::Instant, now: std::time::Instant) -> bool {
        self.event_bus
            .get::<ExploreComponent>(self.explore_component)
            .is_some_and(|explore| explore.jev_progress_expires_between(previous, now))
            || self
                .event_bus
                .get::<OverlayComponent>(self.overlay_component)
                .is_some_and(|overlay| overlay.changes_between(previous, now))
            || self
                .event_bus
                .get::<DiffComponent>(self.diff_component)
                .is_some_and(DiffComponent::has_pending_load)
    }

    /// Create the application and mount its components.
    pub fn new(theme: Theme, file_width: Option<u16>, repository_root: PathBuf) -> Self {
        let palette = theme.palette;
        let mut event_bus = ComponentEventBus::new();
        let reviewable_files = ReviewableFiles::default();
        let files = event_bus.mount(|events| {
            FilesComponent::with_reviewable_files(events, reviewable_files.clone())
        });
        let editor_keymap = KeymapSetting::default();
        let diff = event_bus.mount(|events| {
            DiffComponent::new(
                events,
                reviewable_files.clone(),
                SyntaxHighlighter::new(theme.syntax, theme.palette.text),
                repository_root.clone(),
                theme.palette,
            )
            .with_editor_keymap(editor_keymap.clone())
        });
        let locations = event_bus
            .mount(|events| LocationsComponent::new(events, repository_root, theme.palette));
        let status = event_bus.mount(StatusComponent::new);
        let threads = event_bus.mount(ThreadsComponent::new);
        let explore =
            event_bus.mount(|events| ExploreComponent::with_keymap(events, editor_keymap.clone()));
        let overlay = event_bus.mount(|_| OverlayComponent::new(theme));
        let revision = event_bus.mount(|events| RevisionComponent::new(events, theme.palette));
        let navigation_requests = event_bus.mount(|_| NavigationRequests::default());
        let viewport = Viewport {
            width: 80,
            height: 24,
            file_width,
        };
        Self {
            event_bus,
            hovered_component: None,
            files_component: files,
            threads_component: threads,
            explore_component: explore,
            diff_component: diff,
            locations_component: locations,
            status_component: status,
            overlay_component: overlay,
            revision_component: revision,
            navigation_requests,
            viewport,
            navigation: Navigation::new(viewport),
            palette,
            component_areas: Vec::new(),
            global_input_pending: false,
            application_shortcuts: ui_shortcuts::ShortcutMatcher::new(),
            file_pane_resize: None,
            saved_editor_keymap: editor_keymap.get(),
            editor_keymap,
            #[cfg(test)]
            focused_for_test: None,
        }
    }

    /// Apply the saved keymap shared by every text editor.
    pub fn set_editor_keymap(&mut self, keymap: EditorKeymap) {
        self.editor_keymap.set(keymap);
        self.saved_editor_keymap = keymap;
    }

    /// Deliver one terminal input through the component registry.
    #[allow(clippy::needless_pass_by_value)]
    pub fn update(&mut self, message: UserInput) -> Vec<Action> {
        self.record_viewport_size(&message);
        self.synchronize();
        let Ok(results) = self.dispatch_primary_message(&message) else {
            debug_assert!(false, "the mounted component must match its subscription");
            return Vec::new();
        };
        self.synchronize();
        self.collect_actions(results)
    }

    /// Publish one typed application event to its subscribed components.
    pub fn publish<Event>(&mut self, event: Event) -> Vec<Action>
    where
        Event: ApplicationEvent,
    {
        let Ok(results) = self.event_bus.publish(event) else {
            debug_assert!(false, "the mounted component must match its subscription");
            return Vec::new();
        };
        self.synchronize();
        self.collect_actions(results)
    }

    /// Publish one erased application event to its subscribed components.
    pub fn publish_envelope(&mut self, event: &EventEnvelope) -> Vec<Action> {
        let Ok(results) = self.event_bus.publish_envelope(event.clone()) else {
            debug_assert!(false, "the mounted component must match its subscription");
            return Vec::new();
        };
        self.synchronize();
        self.collect_actions(results)
    }

    fn record_viewport_size(&mut self, message: &UserInput) {
        if let UserInput::Resize { width, height } = message {
            self.viewport.width = *width;
            self.viewport.height = *height;
        }
    }

    fn dispatch_primary_message(
        &mut self,
        message: &UserInput,
    ) -> Result<Vec<DispatchResult<Action>>, DispatchError> {
        match message {
            UserInput::Paste(text) => self
                .event_bus
                .dispatch_input(
                    &EventEnvelope::new(ui_events::TextPasted(text.clone())),
                    self.input_target(),
                )
                .map(component_core::InputDispatch::into_results),
            UserInput::Key(key) => self.dispatch_keyboard_input(&EventEnvelope::new(*key)),
            UserInput::Resize { width, height } => self.dispatch_resize(*width, *height),
            message => self.dispatch_pointer_input(message),
        }
    }

    fn dispatch_pointer_input(
        &mut self,
        message: &UserInput,
    ) -> Result<Vec<DispatchResult<Action>>, DispatchError> {
        if let Some(mode) = self.navigation_tab_at(message) {
            return self.event_bus.publish(ReviewNavigationChanged(mode));
        }
        if let Some(results) = self.dispatch_file_pane_resize(message) {
            return Ok(results);
        }
        let Some(kind) = pointer_kind(message) else {
            return Ok(Vec::new());
        };
        if let Some(resize) = self.file_pane_resize.take() {
            self.hovered_component = None;
            return Ok(resize
                .moved
                .then(|| {
                    vec![Action::Settings(SettingsAction::SaveFilePaneWidth(
                        self.viewport.file_width.unwrap_or_default(),
                    ))]
                    .into_dispatch_result()
                })
                .into_iter()
                .collect());
        }
        let pointer_position = pointer_position(message);
        let component_area =
            pointer_position.and_then(|(column, row)| self.component_area_at(column, row));
        let target = if kind == PointerInputKind::Release {
            self.hovered_component
        } else {
            component_area.map(|component| component.target)
        };
        let Some(target) = target else {
            self.hovered_component = None;
            return Ok(Vec::new());
        };
        if matches!(
            kind,
            PointerInputKind::Click | PointerInputKind::DoubleClick
        ) {
            self.focus_clicked_pane(target);
        }
        let position = pointer_position
            .zip(component_area)
            .map(|((column, row), area)| PointerPosition {
                terminal_column: column,
                terminal_row: row,
                component_column: column.saturating_sub(area.area.x),
                component_row: row.saturating_sub(area.area.y),
            });
        if kind == PointerInputKind::Release {
            self.hovered_component = None;
        } else {
            self.hovered_component = Some(target);
        }
        Ok(self
            .event_bus
            .dispatch_hovered_input(&EventEnvelope::new(PointerInput { kind, position }), target)?
            .into_results())
    }

    fn focus_clicked_pane(&mut self, target: ComponentTarget) {
        let pane = if target == self.diff_component {
            ReviewPane::Detail
        } else if [
            self.files_component,
            self.threads_component,
            self.explore_component,
        ]
        .contains(&target)
        {
            ReviewPane::Navigation
        } else {
            return;
        };
        self.navigation.focus_pane(pane);
    }

    fn navigation_tab_at(&self, message: &UserInput) -> Option<ReviewNavigation> {
        let UserInput::MouseClick { column, row, .. } = message else {
            return None;
        };
        let tabs = self.navigation.layout().tabs()?;
        if self.navigation.modal().is_some() || !tabs.contains((*column, *row).into()) {
            return None;
        }
        let unread = self
            .event_bus
            .get::<ThreadsComponent>(self.threads_component)
            .is_some_and(ThreadsComponent::has_unread_replies);
        NavigationTabs::mode_at(column - tabs.x, unread)
    }

    fn dispatch_resize(
        &mut self,
        width: u16,
        height: u16,
    ) -> Result<Vec<DispatchResult<Action>>, DispatchError> {
        self.event_bus.publish(ViewportChanged { width, height })
    }

    fn collect_actions(&mut self, component_results: Vec<DispatchResult<Action>>) -> Vec<Action> {
        let mut actions: Vec<_> = component_results
            .into_iter()
            .flat_map(DispatchResult::into_actions)
            .collect();
        let keymap = self.editor_keymap.get();
        if keymap != self.saved_editor_keymap {
            self.saved_editor_keymap = keymap;
            actions.push(Action::Settings(SettingsAction::SaveEditorKeymap(keymap)));
        }
        let positions = self
            .event_bus
            .get::<DiffComponent>(self.diff_component)
            .map(DiffComponent::explore_positions)
            .unwrap_or_default();
        if let Ok(saves) = self.event_bus.publish(ui_events::ExploreAutosave {
            positions,
            focus: self.navigation.explore_focus(),
        }) {
            let saves = saves.into_iter().flat_map(DispatchResult::into_actions);
            actions.splice(0..0, saves);
        }
        actions
    }

    /// Return a side-effect-free view of all mounted components.
    ///
    /// # Panics
    ///
    /// Panics if the files component was removed from the application registry.
    pub fn frame(&self) -> ApplicationFrame<'_> {
        ApplicationFrame {
            layout: self.navigation.layout(),
            mode: self.navigation.mode(),
            focus: self.navigation.focus(),
            palette: self.palette,
            files: self
                .event_bus
                .get::<FilesComponent>(self.files_component)
                .expect("the files component must stay mounted"),
            threads: self
                .event_bus
                .get::<ThreadsComponent>(self.threads_component)
                .expect("the threads component must stay mounted"),
            diff: self
                .event_bus
                .get::<DiffComponent>(self.diff_component)
                .expect("the diff component must stay mounted"),
            explore: self
                .event_bus
                .get::<ExploreComponent>(self.explore_component)
                .expect("Explore stays mounted"),
            locations: self
                .event_bus
                .get::<LocationsComponent>(self.locations_component)
                .expect("the locations component must stay mounted"),
            status: self
                .event_bus
                .get::<StatusComponent>(self.status_component)
                .expect("the status component must stay mounted"),
            overlay: self
                .event_bus
                .get::<OverlayComponent>(self.overlay_component)
                .expect("the overlay component must stay mounted"),
            revision: self
                .event_bus
                .get::<RevisionComponent>(self.revision_component)
                .expect("the revision component must stay mounted"),
        }
    }

    fn component_area_at(&self, column: u16, row: u16) -> Option<ComponentArea> {
        self.component_areas
            .iter()
            .rev()
            .find(|component| component.area.contains((column, row).into()))
            .copied()
    }

    fn dispatch_file_pane_resize(
        &mut self,
        message: &UserInput,
    ) -> Option<Vec<DispatchResult<Action>>> {
        match message {
            UserInput::MouseClick { column, row, .. }
                if self.navigation.layout().is_separator(*column, *row) =>
            {
                self.file_pane_resize = Some(FilePaneResize { moved: false });
                Some(Vec::new())
            }
            UserInput::MouseDrag { column, .. } if self.file_pane_resize.is_some() => {
                self.viewport.file_width = Some(
                    PaneLayout::new(self.viewport.width, self.viewport.height, Some(*column))
                        .file_width,
                );
                if let Some(resize) = &mut self.file_pane_resize {
                    resize.moved = true;
                }
                Some(Vec::new())
            }
            _ => None,
        }
    }

    /// Bring navigation up to date with what components requested, which
    /// modals are open and the viewport, then lay the screen out once.
    fn synchronize(&mut self) {
        let requests = self
            .event_bus
            .get_mut::<NavigationRequests>(self.navigation_requests)
            .map(NavigationRequests::take)
            .unwrap_or_default();
        self.navigation.apply(requests);
        self.navigation.set_modals(OpenModals {
            revision: self
                .event_bus
                .get::<RevisionComponent>(self.revision_component)
                .is_some_and(RevisionComponent::is_modal),
            overlay: self
                .event_bus
                .get::<OverlayComponent>(self.overlay_component)
                .is_some_and(OverlayComponent::is_modal),
            locations: self
                .event_bus
                .get::<LocationsComponent>(self.locations_component)
                .is_some_and(LocationsComponent::is_active),
        });
        self.navigation.relayout(self.viewport);
        self.synchronize_component_areas();
    }

    /// Route pointer input and announce pane sizes from the shared layout.
    fn synchronize_component_areas(&mut self) {
        let layout = self.navigation.layout();
        let application_area = Rect::new(0, 0, self.viewport.width, self.viewport.height);
        let mut areas = Vec::new();
        if let Some(area) = layout.navigation_input_area() {
            areas.push(ComponentArea {
                target: self.pane_target(self.navigation.navigation_pane()),
                area,
            });
            let _ = self.event_bus.publish(FilesViewportChanged {
                rows: layout.page_rows(),
            });
        }
        if let Some(area) = layout.visible_diff_pane() {
            areas.push(ComponentArea {
                target: self.diff_component,
                area,
            });
        }
        self.publish_detail_viewport(layout);
        let modals = self.navigation.modals();
        if modals.is_open(Modal::Locations) {
            areas.push(ComponentArea {
                target: self.locations_component,
                area: layout.locations_pane(),
            });
        }
        for area in [layout.header(), layout.footer()] {
            areas.push(ComponentArea {
                target: self.status_component,
                area,
            });
        }
        for modal in [Modal::Overlay, Modal::Revision] {
            if modals.is_open(modal) {
                areas.push(ComponentArea {
                    target: self.modal_target(modal),
                    area: application_area,
                });
            }
        }
        self.component_areas = areas;
    }

    fn publish_detail_viewport(&mut self, layout: ScreenLayout) {
        if let Body::Explore { source, .. } = layout.body() {
            let explore = self
                .event_bus
                .get::<ExploreComponent>(self.explore_component)
                .expect("Explore stays mounted");
            let diff = self
                .event_bus
                .get::<DiffComponent>(self.diff_component)
                .expect("diff stays mounted");
            let viewports = explore.viewports(source, diff, self.palette);
            let _ = self.event_bus.publish(viewports);
        } else if let Some(pane) = layout.diff_pane() {
            let _ = self.event_bus.publish(DiffViewportChanged {
                width: pane.width.saturating_sub(2),
                height: pane.height.saturating_sub(2),
            });
        }
    }

    fn dispatch_keyboard_input(
        &mut self,
        event: &EventEnvelope,
    ) -> Result<Vec<DispatchResult<Action>>, DispatchError> {
        if self.navigation.modal().is_none()
            && self.navigation.focus_is_enclosed()
            && let Some(results) = self
                .event_bus
                .dispatch_enclosing_input(event, self.focused_target())?
        {
            return Ok(results);
        }
        let dispatch = if self.global_input_pending {
            self.event_bus.dispatch_global_input(event)?
        } else {
            self.event_bus.dispatch_input(event, self.input_target())?
        };
        self.global_input_pending = dispatch.global_input_pending();
        let results = dispatch.into_results();
        if !results.is_empty() || self.global_input_pending {
            return Ok(results);
        }
        Ok(self.dispatch_application_shortcut(event))
    }

    fn dispatch_application_shortcut(
        &mut self,
        event: &EventEnvelope,
    ) -> Vec<DispatchResult<Action>> {
        let Some(key) = event.downcast_ref::<crate::Key>() else {
            return Vec::new();
        };
        let InputResolution::Matched(command) = self.application_shortcuts.resolve_key(*key) else {
            return Vec::new();
        };
        match command {
            ui_shortcuts::ApplicationCommand::Search(ui_shortcuts::SearchShortcut::Begin) => {
                self.begin_search(event)
            }
            ui_shortcuts::ApplicationCommand::Application(command) => {
                self.run_application_command(command)
            }
        }
    }

    fn run_application_command(
        &mut self,
        command: ui_shortcuts::ApplicationShortcut,
    ) -> Vec<DispatchResult<Action>> {
        let actions = match command {
            ui_shortcuts::ApplicationShortcut::OpenFiles => {
                return self.change_navigation(ReviewNavigation::Files);
            }
            ui_shortcuts::ApplicationShortcut::OpenThreads => {
                return self.change_navigation(ReviewNavigation::Threads);
            }
            ui_shortcuts::ApplicationShortcut::OpenExplore => {
                return self.change_navigation(ReviewNavigation::Explore);
            }
            ui_shortcuts::ApplicationShortcut::ToggleNavigation => {
                return self.change_navigation(self.navigation.mode().next());
            }
            ui_shortcuts::ApplicationShortcut::NewReplies => {
                return self
                    .event_bus
                    .publish(ui_events::NewRepliesRequested)
                    .unwrap_or_default();
            }
            ui_shortcuts::ApplicationShortcut::ChangeFocus => {
                let focus = self.navigation.toggle_focus();
                return self
                    .event_bus
                    .publish(ui_events::ReviewPaneFocusRequested(focus))
                    .unwrap_or_default();
            }
            ui_shortcuts::ApplicationShortcut::Clear => {
                return self
                    .event_bus
                    .publish(DiffInputClearRequested)
                    .unwrap_or_default();
            }
            ui_shortcuts::ApplicationShortcut::Quit => vec![Action::Terminal(TerminalAction::Quit)],
        };
        vec![actions.into_dispatch_result()]
    }

    fn change_navigation(&mut self, mode: ReviewNavigation) -> Vec<DispatchResult<Action>> {
        self.event_bus
            .publish(ReviewNavigationChanged(mode))
            .unwrap_or_default()
    }

    fn begin_search(&mut self, event: &EventEnvelope) -> Vec<DispatchResult<Action>> {
        let (pane, target) = if self.navigation.mode() == ReviewNavigation::Threads {
            (ReviewPane::Navigation, self.threads_component)
        } else {
            (ReviewPane::Detail, self.diff_component)
        };
        self.navigation.focus_pane(pane);
        self.event_bus
            .dispatch_input(event, target)
            .map(component_core::InputDispatch::into_results)
            .unwrap_or_default()
    }

    /// The component that receives keyboard input: the open modal, otherwise
    /// the focused pane.
    fn input_target(&self) -> ComponentTarget {
        self.navigation
            .modal()
            .map_or_else(|| self.focused_target(), |modal| self.modal_target(modal))
    }

    fn modal_target(&self, modal: Modal) -> ComponentTarget {
        match modal {
            Modal::Revision => self.revision_component,
            Modal::Overlay => self.overlay_component,
            Modal::Locations => self.locations_component,
        }
    }

    fn focused_target(&self) -> ComponentTarget {
        #[cfg(test)]
        if let Some(target) = self.focused_for_test {
            return target;
        }
        self.pane_target(self.navigation.focused_pane())
    }

    fn pane_target(&self, pane: PaneComponent) -> ComponentTarget {
        match pane {
            PaneComponent::Files => self.files_component,
            PaneComponent::Threads => self.threads_component,
            PaneComponent::Explore => self.explore_component,
            PaneComponent::Diff => self.diff_component,
        }
    }

    #[cfg(test)]
    fn mount_input_component_for_test<C>(
        &mut self,
        construct: impl FnOnce(component_core::EventPublisher) -> C,
        focused: bool,
    ) where
        C: component_core::Component<Action>,
    {
        let target = self.event_bus.mount(construct);
        if focused {
            self.focused_for_test = Some(target);
        }
    }
}

fn pointer_position(message: &UserInput) -> Option<(u16, u16)> {
    match message {
        UserInput::MouseScroll { column, row, .. }
        | UserInput::MouseClick { column, row, .. }
        | UserInput::MouseControlClick { column, row }
        | UserInput::MouseDoubleClick { column, row }
        | UserInput::MouseRightClick { column, row }
        | UserInput::MouseDrag { column, row } => Some((*column, *row)),
        UserInput::MouseRelease
        | UserInput::Resize { .. }
        | UserInput::Key(_)
        | UserInput::Paste(_) => None,
    }
}

fn pointer_kind(message: &UserInput) -> Option<PointerInputKind> {
    match message {
        UserInput::MouseScroll { delta, .. } => Some(PointerInputKind::Scroll(*delta)),
        UserInput::MouseClick { .. } => Some(PointerInputKind::Click),
        UserInput::MouseControlClick { .. } => Some(PointerInputKind::ControlClick),
        UserInput::MouseDoubleClick { .. } => Some(PointerInputKind::DoubleClick),
        UserInput::MouseRightClick { .. } => Some(PointerInputKind::RightClick),
        UserInput::MouseDrag { .. } => Some(PointerInputKind::Drag),
        UserInput::MouseRelease => Some(PointerInputKind::Release),
        UserInput::Resize { .. } | UserInput::Key(_) | UserInput::Paste(_) => None,
    }
}

#[cfg(test)]
#[path = "application.tests.rs"]
mod tests;
