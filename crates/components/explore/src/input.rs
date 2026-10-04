use super::navigation::History;
use super::start::{FrontControl, StartButton};
use super::{ComposeScope, Control, EditorTarget, ExploreComponent, Progress, Reveal};
use component_core::{AnyInput, ComponentSubscriptions, InputMatcher, InputResolution, InputScope};
use ui_actions::Action;
use ui_events::{
    EvidenceView, ExploreEvidenceInput, PointerInput, PointerInputKind, ReviewNavigation,
    ReviewPane, ReviewPaneFocusRequested, TextPasted,
};
use ui_shortcuts::{
    ApplicationShortcut, ExploreCommand, ExploreEvidenceShortcut, ExploreGlobalShortcut,
    ExploreShortcut, ExploreTurnShortcut, Key, OverlayShortcut, ShortcutMatcher,
    ShortcutSubscription,
};

pub(super) struct ResizeDrag {
    view: EvidenceView,
    row: u16,
    height: u16,
}

impl ExploreComponent {
    pub(super) fn register_input(subscriptions: &mut ComponentSubscriptions<'_, Self, Action>) {
        subscriptions.subscribe_input(
            InputScope::Focused,
            ExploreKeys::default(),
            |component, input| component.key(input, ReviewPane::Navigation),
        );
        subscriptions.subscribe_input(
            InputScope::Enclosing,
            EvidenceKeys::default(),
            |component, input| component.key(input, ReviewPane::Detail),
        );
        subscriptions.subscribe_input(InputScope::Hovered, AnyInput, Self::pointer);
        subscriptions.subscribe_input(
            InputScope::Focused,
            AnyInput,
            |component: &mut Self, input: TextPasted| {
                component.reset.cancel();
                if component.shows_page_round() {
                    return;
                }
                if component.compose_scope == ComposeScope::Conclusion
                    && component.editor_target == EditorTarget::Implementation
                {
                    component.edit_implementation();
                } else if component.can_compose() {
                    component.edit_answer();
                }
                if component.editing {
                    component.edit_current(|editor| editor.paste(&input.0));
                }
            },
        );
    }

    fn activate(&mut self, control: Control) -> Vec<Action> {
        if matches!(control, Control::Reset | Control::ConfirmReset) {
            return self.reset(std::time::Instant::now());
        }
        // Any other control cancels a Reset waiting for its confirmation.
        self.reset.cancel();
        match control {
            Control::Front(control) => return self.front_control(control),
            Control::Implement => return self.implement(),
            Control::NewImplementation => return self.new_implementation(),
            Control::Send => return self.answer(control),
            Control::Cancel => return self.cancel(),
            Control::Retry => return self.retry(),
            Control::CancelAnswer(index) => return self.cancel_answer(index),
            Control::CancelImplementation => return self.cancel_implementation(),
            control => self.navigate(control),
        }
        Vec::new()
    }

    pub(super) fn cancel(&mut self) -> Vec<Action> {
        if self.awaiting_page_start() {
            return self.stop_page_start();
        }
        if let Some(exploration) = &mut self.exploration {
            exploration.cancel();
        }
        if self.progress.awaiting_capture() {
            self.progress = Progress::DiscardingCapture;
            self.status = "Stopped. Waiting for preparation to finish before Retry.".into();
        } else {
            self.progress = Progress::Retryable;
            self.status =
                "Stopped waiting. Your answer and text remain available; Retry sends them again."
                    .into();
        }
        vec![Action::Explore(review_explore::Command::Cancel)]
    }

    fn navigate(&mut self, control: Control) {
        match control {
            Control::GeneralReply | Control::EditImplementation | Control::Edit => {
                self.begin_edit(control);
            }
            Control::Map | Control::Marks(_) => self.toggle(control),
            Control::Reply(turn) => self.edit_turn(turn),
            Control::History(target) => self.visit_history(target),
            Control::FirstEvidence(_) | Control::Evidence(_) => {
                self.navigate_evidence(control);
            }
            Control::SelectChoice(choice) => self.select_choice(choice),
            _ => {}
        }
    }

    fn navigate_evidence(&mut self, control: Control) {
        match control {
            Control::FirstEvidence(view) => self.open_evidence(view, true),
            Control::Evidence(view) => self.open_evidence(view, false),
            _ => {}
        }
    }

    pub(super) fn edit_answer(&mut self) {
        self.editor_target = EditorTarget::Answer;
        self.editing = self.can_compose();
        self.evidence_list_focused = false;
        if self.editing {
            self.reveal.set(Some(Reveal::Editor(EditorTarget::Answer)));
        }
    }

    fn begin_edit(&mut self, control: Control) {
        match control {
            Control::GeneralReply => self.edit_general(),
            Control::EditImplementation => self.edit_implementation(),
            _ => self.edit_answer(),
        }
    }

    fn open_evidence(&mut self, view: EvidenceView, reveal: bool) {
        let EvidenceView { turn, reference } = view;
        if let Some(turn) = self.turns.get_mut(turn) {
            turn.reference = reference;
            self.publish_evidence(view, reveal);
        }
    }

    /// Move focus on from `focus`, the pane that had it when Tab was pressed.
    fn cycle_focus(&mut self, focus: ReviewPane) {
        let from_evidence = focus == ReviewPane::Detail;
        if !self.can_compose() {
            return;
        }
        if self.general_reply() {
            if self.editing && !from_evidence {
                self.editing = false;
            } else if self.conclusion().is_some_and(|view| view.replying) {
                self.edit_answer();
            } else {
                self.edit_implementation();
            }
            self.events
                .publish(ReviewPaneFocusRequested(ReviewPane::Navigation));
            return;
        }
        if self.question().is_none() {
            return;
        }
        let pane = if from_evidence {
            self.editing = false;
            self.evidence_list_focused = true;
            self.reveal.set(Some(Reveal::Evidence));
            ReviewPane::Navigation
        } else if self.evidence_list_focused {
            self.edit_answer();
            ReviewPane::Navigation
        } else if self.editing {
            self.editing = false;
            ReviewPane::Navigation
        } else {
            self.reveal.set(Some(Reveal::Evidence));
            self.publish_evidence(self.view_id(), false);
            ReviewPane::Detail
        };
        self.events.publish(ReviewPaneFocusRequested(pane));
    }

    /// Run one key Explore owns. `focus` is the pane that had focus: the
    /// conversation, or the evidence Explore shows inside itself.
    fn key(&mut self, input: ExploreKey, focus: ReviewPane) -> Vec<Action> {
        // Reset has no key, so any key cancels a Reset waiting for its confirmation.
        self.reset.cancel();
        if self.shows_page_round() {
            return self.page_round_key(input.command);
        }
        if input.command == Some(ExploreCommand::Global(ExploreGlobalShortcut::CycleFocus)) {
            self.cycle_focus(focus);
            return Vec::new();
        }
        if let Some(actions) = self.focused_evidence_key(input) {
            return actions;
        }
        match input.command {
            Some(ExploreCommand::Global(ExploreGlobalShortcut::GrowEvidence)) => self.resize_by(2),
            Some(ExploreCommand::Global(ExploreGlobalShortcut::ShrinkEvidence)) => {
                self.resize_by(-2);
            }
            Some(ExploreCommand::Global(ExploreGlobalShortcut::FitEvidence)) => self.fit_evidence(),
            Some(ExploreCommand::Explore(ExploreShortcut::Send)) => {
                return self.activate(self.send_control());
            }
            _ if self.editing => self.edit_current(|editor| editor.input(input.key)),
            Some(ExploreCommand::Explore(command)) => return self.command(command),
            _ => {}
        }
        Vec::new()
    }

    fn fit_evidence(&mut self) {
        let view = self.view_id();
        self.heights.remove(&view);
        self.open_evidence(view, true);
    }

    fn focused_evidence_key(&mut self, input: ExploreKey) -> Option<Vec<Action>> {
        self.evidence_list_focused
            .then(|| self.evidence_list_key(input))
            .flatten()
    }

    fn send_control(&self) -> Control {
        if self.compose_scope == ComposeScope::Conclusion
            && self.editor_target == EditorTarget::Implementation
        {
            Control::Implement
        } else {
            Control::Send
        }
    }

    fn evidence_list_key(&mut self, input: ExploreKey) -> Option<Vec<Action>> {
        match input.command {
            Some(ExploreCommand::Explore(ExploreShortcut::Confirm)) => {
                self.evidence_list_focused = false;
                self.events
                    .publish(ReviewPaneFocusRequested(ReviewPane::Detail));
                return Some(Vec::new());
            }
            Some(ExploreCommand::Explore(ExploreShortcut::Back)) => {
                self.evidence_list_focused = false;
                self.evidence_keys = ShortcutMatcher::new();
                return Some(Vec::new());
            }
            _ => {}
        }
        match self.evidence_keys.resolve_key(input.key) {
            InputResolution::AwaitingMoreInput => Some(Vec::new()),
            InputResolution::Matched(input) => {
                let view = self.layout.borrow().navigate_evidence(input);
                if let Some(view) = view {
                    self.open_evidence(view, true);
                }
                Some(Vec::new())
            }
            InputResolution::NoMatch => None,
        }
    }

    fn command(&mut self, command: ExploreShortcut) -> Vec<Action> {
        if let Some(control) = self
            .choice_control(command)
            .or_else(|| self.control(command))
        {
            return self.activate(control);
        }
        let delta = match command {
            ExploreShortcut::SelectNext | ExploreShortcut::ScrollDown => 5,
            ExploreShortcut::SelectPrevious | ExploreShortcut::ScrollUp => -5,
            _ => return Vec::new(),
        };
        self.scroll_by(delta);
        Vec::new()
    }

    fn control(&self, command: ExploreShortcut) -> Option<Control> {
        match command {
            ExploreShortcut::Confirm if self.compose_scope == ComposeScope::Conclusion => {
                Some(Control::EditImplementation)
            }
            ExploreShortcut::Confirm => Some(Control::Reply(self.selected)),
            ExploreShortcut::ChooseAnswer(choice) => Some(Control::SelectChoice(choice)),
            ExploreShortcut::Turn(command) => Some(Self::turn_control(command)),
            ExploreShortcut::Evidence(command) => Some(self.evidence_control(command)),
            _ => None,
        }
    }

    fn turn_control(command: ExploreTurnShortcut) -> Control {
        match command {
            ExploreTurnShortcut::Start(shortcut) => {
                Control::Front(FrontControl::Start(StartButton::of(shortcut).start))
            }
            ExploreTurnShortcut::OpenPage => Control::Front(FrontControl::OpenPage),
            ExploreTurnShortcut::ContinueInPane => Control::Front(FrontControl::ContinueInPane),
            ExploreTurnShortcut::Cancel => Control::Cancel,
            ExploreTurnShortcut::Retry => Control::Retry,
            ExploreTurnShortcut::PreviousTurn => Control::History(History::Previous),
            ExploreTurnShortcut::NextTurn => Control::History(History::Next),
            ExploreTurnShortcut::ToggleMap => Control::Map,
        }
    }

    fn evidence_control(&self, command: ExploreEvidenceShortcut) -> Control {
        match command {
            ExploreEvidenceShortcut::First => Control::FirstEvidence(EvidenceView {
                turn: self.selected,
                reference: 0,
            }),
            ExploreEvidenceShortcut::Next => {
                let count = self
                    .exploration
                    .as_ref()
                    .map_or(0, |exploration| exploration.evidence(self.selected).len());
                Control::Evidence(EvidenceView {
                    turn: self.selected,
                    reference: (self
                        .turns
                        .get(self.selected)
                        .map_or(0, |turn| turn.reference)
                        + 1)
                        % count.max(1),
                })
            }
        }
    }

    fn scroll_by(&mut self, delta: isize) {
        self.editing = false;
        self.events
            .publish(ReviewPaneFocusRequested(ReviewPane::Navigation));
        self.reveal.set(None);
        self.scroll.set(
            self.scroll
                .get()
                .saturating_add_signed(delta)
                .min(self.layout.borrow().maximum_scroll()),
        );
    }

    fn resize_by(&mut self, delta: i16) {
        let view = self.view_id();
        let height = self.layout.borrow().window_height(view);
        if let Some(height) = height {
            self.heights
                .insert(view, height.saturating_add_signed(delta).max(5));
        }
    }

    fn resize_pointer(&mut self, input: PointerInput) -> bool {
        if let Some(drag) = &self.drag {
            if let Some(position) = input.position {
                let delta = i32::from(position.terminal_row) - i32::from(drag.row);
                self.heights.insert(
                    drag.view,
                    u16::try_from((i32::from(drag.height) + delta).max(5)).unwrap_or(u16::MAX),
                );
            }
            if input.kind == PointerInputKind::Release {
                self.drag = None;
            }
            return true;
        }
        false
    }

    fn split_pointer(&mut self, input: PointerInput) -> bool {
        if self.split_drag {
            if input.kind == PointerInputKind::Drag
                && let Some(position) = input.position
            {
                let origin = self.layout.borrow().area.x;
                self.evidence_width = Some(position.terminal_column.saturating_sub(origin));
            }
            if input.kind == PointerInputKind::Release {
                self.split_drag = false;
            }
            return true;
        }
        if input.kind == PointerInputKind::Click
            && let Some(position) = input.position
            && self
                .layout
                .borrow()
                .evidence_divider_at(position.terminal_column, position.terminal_row)
        {
            self.split_drag = true;
            return true;
        }
        false
    }

    fn pointer_resize(&mut self, input: PointerInput) -> bool {
        self.split_pointer(input) || self.resize_pointer(input)
    }

    fn pointer(&mut self, input: PointerInput) -> Vec<Action> {
        if matches!(input.kind, PointerInputKind::Click) && !self.clicks_reset(input) {
            self.reset.cancel();
        }
        self.pointer_conversation(input)
    }

    fn clicks_reset(&self, input: PointerInput) -> bool {
        input.position.is_some_and(|position| {
            matches!(
                self.layout
                    .borrow()
                    .navigation
                    .control_at(position.terminal_column, position.terminal_row),
                Some(Control::Reset | Control::ConfirmReset)
            )
        })
    }

    fn pointer_conversation(&mut self, input: PointerInput) -> Vec<Action> {
        if self.pointer_resize(input) {
            return Vec::new();
        }
        if matches!(
            input.kind,
            PointerInputKind::Drag | PointerInputKind::Release
        ) && let Some(window) = self.pointer_view
        {
            self.viewer_pointer(window, input);
            return Vec::new();
        }
        let Some(position) = input.position else {
            return Vec::new();
        };
        let column = position.terminal_column;
        let row = position.terminal_row;
        let resize = self.layout.borrow().resize_at(column, row);
        if matches!(input.kind, PointerInputKind::Click)
            && let Some((view, height)) = resize
        {
            self.open_evidence(view, false);
            self.events
                .publish(ReviewPaneFocusRequested(ReviewPane::Detail));
            self.drag = Some(ResizeDrag { view, row, height });
            return Vec::new();
        }
        let window = self.layout.borrow().window_at(column, row);
        if let Some(window) = window {
            self.viewer_pointer(window, input);
            return Vec::new();
        }
        if let PointerInputKind::Scroll(delta) = input.kind {
            return self.scroll_conversation_at(column, row, delta);
        }
        if let Some(actions) = self.click_conversation_control(input.kind, column, row) {
            return actions;
        }
        Vec::new()
    }

    fn click_conversation_control(
        &mut self,
        kind: PointerInputKind,
        column: u16,
        row: u16,
    ) -> Option<Vec<Action>> {
        if !matches!(kind, PointerInputKind::Click) {
            return None;
        }
        let control = self.layout.borrow().control_at(column, row)?;
        if matches!(control, Control::Evidence(_)) {
            self.editing = false;
            self.evidence_list_focused = true;
        }
        self.events
            .publish(ReviewPaneFocusRequested(ReviewPane::Navigation));
        Some(self.activate(control))
    }

    fn scroll_conversation_at(&mut self, column: u16, row: u16, delta: isize) -> Vec<Action> {
        let evidence = self.layout.borrow().evidence_scroll_at(column, row, delta);
        if let Some(view) = evidence {
            self.open_evidence(view, false);
        } else {
            self.scroll_by(delta);
        }
        Vec::new()
    }

    fn viewer_pointer(&mut self, window: super::flow::Window, mut input: PointerInput) {
        self.open_evidence(window.view, false);
        if matches!(input.kind, PointerInputKind::Click) {
            self.pointer_view = Some(window);
            self.events
                .publish(ReviewPaneFocusRequested(ReviewPane::Detail));
        }
        if let Some(position) = &mut input.position {
            (position.component_column, position.component_row) = window
                .viewport
                .local_position(position.terminal_column, position.terminal_row);
        }
        self.events.publish(ExploreEvidenceInput {
            view: window.view,
            input,
        });
        if input.kind == PointerInputKind::Release {
            self.pointer_view = None;
        }
    }
}

impl ExploreComponent {
    /// Whether `key` runs an application command Explore leaves to the
    /// application. Switching navigation always passes; opening Files,
    /// Threads or help and quitting pass unless an answer is being composed.
    fn passes_through(&self, key: Key) -> bool {
        match ApplicationShortcut::bound_to(key) {
            Some(ApplicationShortcut::ToggleNavigation) => return true,
            Some(
                ApplicationShortcut::OpenFiles
                | ApplicationShortcut::OpenThreads
                | ApplicationShortcut::Quit,
            ) => return !self.editing,
            _ => {}
        }
        !self.editing && OverlayShortcut::bound_to(key) == Some(OverlayShortcut::OpenHelp)
    }
}

/// One key Explore handles, with the Explore command it is bound to, if any.
///
/// The raw key stays available for the answer editor and the lists Explore
/// embeds, which resolve keys through their own subscriptions.
#[derive(Clone, Copy)]
pub(super) struct ExploreKey {
    pub(super) key: Key,
    pub(super) command: Option<ExploreCommand>,
}

/// Keys for the focused Explore conversation. Explore keeps every key except
/// the application's navigation, help and quit keys, so unbound keys never
/// reach the diff behind it.
#[derive(Default)]
struct ExploreKeys {
    commands: ShortcutMatcher<ExploreCommand>,
}

impl InputMatcher<ExploreComponent, Key> for ExploreKeys {
    type Output = ExploreKey;

    fn resolve(&mut self, component: &ExploreComponent, key: &Key) -> InputResolution<ExploreKey> {
        if component.passes_through(*key) {
            return InputResolution::NoMatch;
        }
        let command = match self.commands.resolve_key(*key) {
            InputResolution::Matched(command) => Some(command),
            _ => None,
        };
        InputResolution::Matched(ExploreKey { key: *key, command })
    }
}

/// Keys Explore claims while its evidence, shown inside it, has focus.
#[derive(Default)]
struct EvidenceKeys {
    commands: ShortcutMatcher<ExploreGlobalShortcut>,
}

impl InputMatcher<ExploreComponent, Key> for EvidenceKeys {
    type Output = ExploreKey;

    fn resolve(&mut self, component: &ExploreComponent, key: &Key) -> InputResolution<ExploreKey> {
        if component.mode != ReviewNavigation::Explore {
            return InputResolution::NoMatch;
        }
        self.commands.resolve_key(*key).map(|command| ExploreKey {
            key: *key,
            command: Some(ExploreCommand::Global(command)),
        })
    }
}
