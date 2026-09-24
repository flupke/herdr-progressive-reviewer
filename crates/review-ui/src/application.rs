//! Application coordination for mounted review UI components.

use std::path::PathBuf;

use component_core::{
    ApplicationEvent, ComponentEventBus, ComponentTarget, DispatchError, DispatchResult,
    EventEnvelope, InputResolution, IntoDispatchResult,
};
use diff_component::{DiffComponent, SyntaxHighlighter};
use explore_component::ExploreComponent;
use files_component::FilesComponent;
use guide_component::GuideComponent;
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

use crate::layout::{NavigationTabs, PaneLayout};
use crate::{Action, ApplicationFrame, Theme, UserInput};
use ui_theme::Palette;

/// The root coordinator for the review UI.
pub struct ReviewApplication {
    event_bus: ComponentEventBus<Action>,
    modal_component: Option<ComponentTarget>,
    focused_component: ComponentTarget,
    hovered_component: Option<ComponentTarget>,
    files_component: ComponentTarget,
    threads_component: ComponentTarget,
    explore_component: ComponentTarget,
    diff_component: ComponentTarget,
    guide_component: ComponentTarget,
    locations_component: ComponentTarget,
    status_component: ComponentTarget,
    overlay_component: ComponentTarget,
    revision_component: ComponentTarget,
    file_width: Option<u16>,
    focus: ReviewPane,
    navigation: ReviewNavigation,
    files_focus: ReviewPane,
    threads_focus: ReviewPane,
    explore_focus: ReviewPane,
    consumed_focus_request: u64,
    palette: Palette,
    width: u16,
    height: u16,
    component_areas: Vec<ComponentArea>,
    global_input_pending: bool,
    application_shortcuts: ui_shortcuts::ShortcutMatcher,
    file_pane_resize: Option<FilePaneResize>,
    watched_source: Option<PathBuf>,
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
                .get::<StatusComponent>(self.status_component)
                .is_some_and(StatusComponent::is_animating)
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
        let diff = event_bus.mount(|events| {
            DiffComponent::new(
                events,
                reviewable_files.clone(),
                SyntaxHighlighter::new(theme.syntax, theme.palette.text),
                repository_root.clone(),
                theme.palette,
            )
        });
        let guide = event_bus.mount(GuideComponent::new);
        let locations = event_bus
            .mount(|events| LocationsComponent::new(events, repository_root, theme.palette));
        let status = event_bus.mount(StatusComponent::new);
        let threads = event_bus.mount(ThreadsComponent::new);
        let explore = event_bus.mount(ExploreComponent::new);
        let overlay = event_bus.mount(|_| OverlayComponent::new(theme));
        let revision = event_bus.mount(|events| RevisionComponent::new(events, theme.palette));
        Self {
            event_bus,
            modal_component: None,
            focused_component: files,
            hovered_component: None,
            files_component: files,
            threads_component: threads,
            explore_component: explore,
            diff_component: diff,
            guide_component: guide,
            locations_component: locations,
            status_component: status,
            overlay_component: overlay,
            revision_component: revision,
            file_width,
            focus: ReviewPane::Navigation,
            navigation: ReviewNavigation::Files,
            files_focus: ReviewPane::Navigation,
            threads_focus: ReviewPane::Navigation,
            explore_focus: ReviewPane::Navigation,
            consumed_focus_request: 0,
            palette,
            width: 80,
            height: 24,
            component_areas: Vec::new(),
            global_input_pending: false,
            application_shortcuts: ui_shortcuts::ShortcutMatcher::new(
                ui_shortcuts::ShortcutSet::Application,
            ),
            file_pane_resize: None,
            watched_source: None,
        }
    }

    /// Deliver one terminal input through the component registry.
    #[allow(clippy::needless_pass_by_value)]
    pub fn update(&mut self, message: UserInput) -> Vec<Action> {
        self.record_viewport_size(&message);
        self.synchronize_input_routing();
        self.synchronize_component_areas();
        let Ok(results) = self.dispatch_primary_message(&message) else {
            debug_assert!(false, "the mounted component must match its subscription");
            return Vec::new();
        };
        self.synchronize_input_routing();
        self.synchronize_component_areas();
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
        self.synchronize_input_routing();
        self.synchronize_component_areas();
        self.collect_actions(results)
    }

    /// Publish one erased application event to its subscribed components.
    pub fn publish_envelope(&mut self, event: &EventEnvelope) -> Vec<Action> {
        let Ok(results) = self.event_bus.publish_envelope(event.clone()) else {
            debug_assert!(false, "the mounted component must match its subscription");
            return Vec::new();
        };
        self.synchronize_input_routing();
        self.synchronize_component_areas();
        self.collect_actions(results)
    }

    fn record_viewport_size(&mut self, message: &UserInput) {
        if let UserInput::Resize { width, height } = message {
            self.width = *width;
            self.height = *height;
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
                    self.modal_component.unwrap_or(self.focused_component),
                )
                .map(component_core::InputDispatch::into_results),
            UserInput::Key(key) => self.dispatch_key(*key),
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
                    vec![Action::SaveFilePaneWidth(
                        self.file_width.unwrap_or_default(),
                    )]
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
            if [
                self.files_component,
                self.threads_component,
                self.explore_component,
            ]
            .contains(&target)
            {
                self.set_focus(ReviewPane::Navigation);
                self.focused_component = target;
            } else if target == self.diff_component {
                self.set_focus(ReviewPane::Detail);
                self.focused_component = self.diff_component;
            }
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

    fn navigation_tab_at(&self, message: &UserInput) -> Option<ReviewNavigation> {
        if self.modal_component.is_none()
            && let UserInput::MouseClick { column, row: 1, .. } = message
        {
            let layout = PaneLayout::for_navigation(
                self.width,
                self.height,
                self.file_width,
                self.navigation,
            );
            let width = if layout.is_wide() {
                layout.file_width
            } else {
                self.width
            };
            if (layout.is_wide()
                || self.focus == ReviewPane::Navigation
                || self.navigation == ReviewNavigation::Explore)
                && *column > 0
                && *column < width.saturating_sub(1)
            {
                let unread = self
                    .event_bus
                    .get::<ThreadsComponent>(self.threads_component)
                    .is_some_and(ThreadsComponent::has_unread_replies);
                return NavigationTabs::mode_at(column - 1, unread);
            }
        }
        None
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
        let source = self
            .event_bus
            .get::<DiffComponent>(self.diff_component)
            .and_then(DiffComponent::live_source_path);
        if source != self.watched_source.as_deref() {
            self.watched_source = source.map(std::path::Path::to_owned);
            actions.push(Action::WatchSource(self.watched_source.clone()));
        }
        let positions = self
            .event_bus
            .get::<DiffComponent>(self.diff_component)
            .map(DiffComponent::explore_positions)
            .unwrap_or_default();
        if let Ok(saves) = self.event_bus.publish(ui_events::ExploreAutosave {
            positions,
            focus: (self.navigation == ReviewNavigation::Explore).then_some(self.focus),
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
            file_width: self.file_width,
            focus: self.focus,
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
            guide: self
                .event_bus
                .get::<GuideComponent>(self.guide_component)
                .expect("the guide component must stay mounted"),
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
        let layout =
            PaneLayout::for_navigation(self.width, self.height, self.file_width, self.navigation);
        match message {
            UserInput::MouseClick { column, row, .. } if layout.is_separator(*column, *row) => {
                self.file_pane_resize = Some(FilePaneResize { moved: false });
                Some(Vec::new())
            }
            UserInput::MouseDrag { column, .. } if self.file_pane_resize.is_some() => {
                self.file_width =
                    Some(PaneLayout::new(self.width, self.height, Some(*column)).file_width);
                if let Some(resize) = &mut self.file_pane_resize {
                    resize.moved = true;
                }
                Some(Vec::new())
            }
            _ => None,
        }
    }

    fn set_focus(&mut self, focus: ReviewPane) {
        self.focus = focus;
    }

    fn dispatch_key(
        &mut self,
        key: crate::Key,
    ) -> Result<Vec<DispatchResult<Action>>, DispatchError> {
        self.dispatch_keyboard_input(&EventEnvelope::new(key))
    }

    fn synchronize_input_routing(&mut self) {
        self.synchronize_navigation();
        let overlay_is_modal = self
            .event_bus
            .get::<OverlayComponent>(self.overlay_component)
            .is_some_and(OverlayComponent::is_modal);
        let revision_is_modal = self
            .event_bus
            .get::<RevisionComponent>(self.revision_component)
            .is_some_and(RevisionComponent::is_modal);
        let locations_is_modal = self
            .event_bus
            .get::<LocationsComponent>(self.locations_component)
            .is_some_and(LocationsComponent::is_active);
        self.modal_component = if revision_is_modal {
            Some(self.revision_component)
        } else if overlay_is_modal {
            Some(self.overlay_component)
        } else if locations_is_modal {
            Some(self.locations_component)
        } else {
            None
        };
        if self.modal_component.is_none()
            && matches!(
                self.focused_component,
                target if target == self.diff_component || target == self.files_component || target == self.threads_component || target == self.explore_component
            )
        {
            self.focused_component = match self.focus {
                ReviewPane::Navigation => self.navigation_component(),
                ReviewPane::Detail => self.diff_component,
            };
        }
    }

    fn synchronize_navigation(&mut self) {
        let threads = self
            .event_bus
            .get::<ThreadsComponent>(self.threads_component)
            .expect("the threads component must stay mounted");
        let mode = threads.mode();
        let (serial, pane) = threads.focus_request();
        if self.navigation != mode {
            match self.navigation {
                ReviewNavigation::Files => self.files_focus = self.focus,
                ReviewNavigation::Threads => self.threads_focus = self.focus,
                ReviewNavigation::Explore => self.explore_focus = self.focus,
            }
            self.focus = match mode {
                ReviewNavigation::Files => self.files_focus,
                ReviewNavigation::Threads => self.threads_focus,
                ReviewNavigation::Explore => self.explore_focus,
            };
            self.navigation = mode;
        }
        if serial != self.consumed_focus_request {
            self.consumed_focus_request = serial;
            self.focus = pane;
        }
    }

    fn synchronize_component_areas(&mut self) {
        let width = self.width;
        let height = self.height;
        let layout = PaneLayout::for_navigation(width, height, self.file_width, self.navigation);
        let files_area = if self.navigation == ReviewNavigation::Explore {
            Some(Rect::new(0, 2, width, height.saturating_sub(3)))
        } else {
            layout.files_content_area(self.focus)
        };
        let files_pane_width = if layout.is_wide() {
            layout.file_width
        } else {
            width
        };
        let application_area = Rect::new(0, 0, width, height);
        let navigation_component = self.navigation_component();
        self.component_areas.retain(|component| {
            component.target != self.files_component
                && component.target != self.threads_component
                && component.target != self.explore_component
        });
        if let Some(area) = files_area {
            self.set_component_area(navigation_component, area, 0);
            let _ = self.event_bus.publish(FilesViewportChanged {
                rows: layout.page_rows(),
            });
        } else {
            self.component_areas
                .retain(|component| component.target != self.files_component);
        }
        self.synchronize_diff_area(layout);
        self.component_areas
            .retain(|component| component.target != self.locations_component);
        if self
            .event_bus
            .get::<LocationsComponent>(self.locations_component)
            .is_some_and(LocationsComponent::is_active)
        {
            self.component_areas.push(ComponentArea {
                target: self.locations_component,
                area: Rect::new(0, 1, files_pane_width, height.saturating_sub(2)),
            });
        }
        self.component_areas
            .retain(|component| component.target != self.status_component);
        self.component_areas.push(ComponentArea {
            target: self.status_component,
            area: Rect::new(0, 0, width, 1),
        });
        self.component_areas.push(ComponentArea {
            target: self.status_component,
            area: Rect::new(0, height.saturating_sub(1), width, 1),
        });
        self.component_areas
            .retain(|component| component.target != self.overlay_component);
        if self
            .event_bus
            .get::<OverlayComponent>(self.overlay_component)
            .is_some_and(OverlayComponent::is_modal)
        {
            self.component_areas.push(ComponentArea {
                target: self.overlay_component,
                area: application_area,
            });
        }
        self.component_areas
            .retain(|component| component.target != self.revision_component);
        if self
            .event_bus
            .get::<RevisionComponent>(self.revision_component)
            .is_some_and(RevisionComponent::is_modal)
        {
            self.component_areas.push(ComponentArea {
                target: self.revision_component,
                area: application_area,
            });
        }
    }

    fn synchronize_diff_area(&mut self, layout: PaneLayout) {
        let width = self.width;
        let height = self.height;
        let diff_pane_area = if layout.is_wide() {
            Rect::new(
                layout.file_width,
                1,
                width.saturating_sub(layout.file_width),
                layout.body_height(),
            )
        } else {
            Rect::new(0, 1, width, layout.body_height())
        };
        self.component_areas
            .retain(|component| component.target != self.diff_component);
        if self.navigation != ReviewNavigation::Explore
            && (layout.is_wide() || self.focus == ReviewPane::Detail)
        {
            self.set_component_area(self.diff_component, diff_pane_area, 1);
        }
        if self.navigation == ReviewNavigation::Explore {
            let explore = self
                .event_bus
                .get::<ExploreComponent>(self.explore_component)
                .expect("Explore stays mounted");
            let diff = self
                .event_bus
                .get::<DiffComponent>(self.diff_component)
                .expect("diff stays mounted");
            let viewports = explore
                .conversation_layout(
                    Rect::new(0, 2, width, height.saturating_sub(3)),
                    diff,
                    self.palette,
                )
                .viewports();
            let _ = self.event_bus.publish(viewports);
        } else {
            let _ = self.event_bus.publish(DiffViewportChanged {
                width: diff_pane_area.width.saturating_sub(2),
                height: diff_pane_area.height.saturating_sub(2),
            });
        }
    }

    fn set_component_area(&mut self, target: ComponentTarget, area: Rect, insert_at: usize) {
        if let Some(component) = self
            .component_areas
            .iter_mut()
            .find(|component| component.target == target)
        {
            component.area = area;
        } else {
            self.component_areas.insert(
                insert_at.min(self.component_areas.len()),
                ComponentArea { target, area },
            );
        }
    }

    fn dispatch_keyboard_input(
        &mut self,
        event: &EventEnvelope,
    ) -> Result<Vec<DispatchResult<Action>>, DispatchError> {
        if self.navigation == ReviewNavigation::Explore && self.modal_component.is_none() {
            match event.downcast_ref::<crate::Key>() {
                Some(crate::Key::Tab) => {
                    return self.event_bus.publish(ui_events::ExploreFocusCycle {
                        from_evidence: self.focus == ReviewPane::Detail,
                    });
                }
                Some(crate::Key::Alt('j' | 'k' | '0')) => {
                    return self
                        .event_bus
                        .dispatch_input(event, self.explore_component)
                        .map(component_core::InputDispatch::into_results);
                }
                _ => {}
            }
        }
        let focused_target = self.modal_component.unwrap_or(self.focused_component);
        let dispatch = if self.global_input_pending {
            self.event_bus.dispatch_global_input(event)?
        } else {
            self.event_bus.dispatch_input(event, focused_target)?
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
        if command == ui_shortcuts::ShortcutCommand::Search(ui_shortcuts::SearchShortcut::Begin) {
            return self.begin_search(event);
        }
        let ui_shortcuts::ShortcutCommand::Application(command) = command else {
            unreachable!("the application shortcut set is exact");
        };
        self.run_application_command(command)
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
            ui_shortcuts::ApplicationShortcut::ToggleNavigation => {
                return self.change_navigation(self.navigation.next());
            }
            ui_shortcuts::ApplicationShortcut::NewReplies => {
                return self
                    .event_bus
                    .publish(ui_events::NewRepliesRequested)
                    .unwrap_or_default();
            }
            ui_shortcuts::ApplicationShortcut::ChangeFocus => {
                self.change_focus();
                return self
                    .event_bus
                    .publish(ui_events::ReviewPaneFocusRequested(self.focus))
                    .unwrap_or_default();
            }
            ui_shortcuts::ApplicationShortcut::Clear => {
                return self
                    .event_bus
                    .publish(DiffInputClearRequested)
                    .unwrap_or_default();
            }
            ui_shortcuts::ApplicationShortcut::Quit => vec![Action::Quit],
            _ => unreachable!("the application shortcut set is exact"),
        };
        vec![actions.into_dispatch_result()]
    }

    fn change_navigation(&mut self, mode: ReviewNavigation) -> Vec<DispatchResult<Action>> {
        self.event_bus
            .publish(ReviewNavigationChanged(mode))
            .unwrap_or_default()
    }

    fn begin_search(&mut self, event: &EventEnvelope) -> Vec<DispatchResult<Action>> {
        if self.navigation == ReviewNavigation::Threads {
            self.focus = ReviewPane::Navigation;
            self.focused_component = self.threads_component;
            return self
                .event_bus
                .dispatch_input(event, self.threads_component)
                .map(component_core::InputDispatch::into_results)
                .unwrap_or_default();
        }
        self.set_focus(ReviewPane::Detail);
        self.focused_component = self.diff_component;
        self.event_bus
            .dispatch_input(event, self.diff_component)
            .map(component_core::InputDispatch::into_results)
            .unwrap_or_default()
    }

    fn change_focus(&mut self) {
        self.focus = match self.focus {
            ReviewPane::Navigation => ReviewPane::Detail,
            ReviewPane::Detail => ReviewPane::Navigation,
        };
        self.focused_component = match self.focus {
            ReviewPane::Navigation => self.navigation_component(),
            ReviewPane::Detail => self.diff_component,
        };
    }

    fn navigation_component(&self) -> ComponentTarget {
        match self.navigation {
            ReviewNavigation::Files => self.files_component,
            ReviewNavigation::Threads => self.threads_component,
            ReviewNavigation::Explore => self.explore_component,
        }
    }

    #[cfg(test)]
    fn mount_input_component_for_test<C>(
        &mut self,
        construct: impl FnOnce(component_core::EventPublisher) -> C,
        area: Rect,
        focused: bool,
    ) where
        C: component_core::Component<Action>,
    {
        let target = self.event_bus.mount(construct);
        self.component_areas.push(ComponentArea { target, area });
        if focused {
            self.focused_component = target;
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
