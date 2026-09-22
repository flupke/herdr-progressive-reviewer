use super::TurnControls;
use super::navigation::History;
use super::{ComposeScope, Control, EditorTarget, ExploreComponent, Progress, Reveal};
use component_core::{AnyInput, ComponentSubscriptions, InputMatcher, InputResolution, InputScope};
use ui_actions::Action;
use ui_events::{
    EvidenceView, ExploreEvidenceInput, ExploreFocusCycle, PointerInput, PointerInputKind,
    ReviewPane, ReviewPaneFocusRequested, TextPasted,
};
use ui_shortcuts::Key;

pub(super) struct ResizeDrag {
    view: EvidenceView,
    row: u16,
    height: u16,
}

impl ExploreComponent {
    pub(super) fn register_input(subscriptions: &mut ComponentSubscriptions<'_, Self, Action>) {
        subscriptions.subscribe(Self::cycle_focus);
        subscriptions.subscribe_input(InputScope::Focused, ExploreKeys, Self::key);
        subscriptions.subscribe_input(InputScope::Hovered, AnyInput, Self::pointer);
        subscriptions.subscribe_input(
            InputScope::Focused,
            AnyInput,
            |component: &mut Self, input: TextPasted| {
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
        if !matches!(control, Control::Start) {
            self.reset_warning = false;
        }
        match control {
            Control::Start => return self.start(),
            Control::PreviousPass | Control::LatestPass => {
                return self.open_saved(control);
            }
            Control::Implement | Control::NewImplementation => {
                if matches!(control, Control::NewImplementation) {
                    self.new_implementation();
                }
                return self.implement();
            }
            Control::Send | Control::Defer => return self.answer(control),
            Control::Cancel => return self.cancel(),
            Control::Retry => return self.retry(),
            Control::CancelImplementation => return self.cancel_implementation(),
            control => self.navigate(control),
        }
        Vec::new()
    }

    fn cancel(&mut self) -> Vec<Action> {
        if let Some(exploration) = &mut self.exploration {
            exploration.cancel();
        }
        if self.progress.awaiting_capture() {
            self.progress = Progress::DiscardingCapture;
            self.status = "Cancelled. Waiting for preparation to finish before Retry.".into();
        } else {
            self.progress = Progress::Retryable;
            self.status = "Cancelled. Your answer and text remain available.".into();
        }
        vec![Action::Explore(review_explore::Command::Cancel)]
    }

    fn navigate(&mut self, control: Control) {
        match control {
            Control::GeneralReply | Control::EditImplementation | Control::Edit => {
                self.begin_edit(control);
            }
            Control::Map => self.map = !self.map,
            Control::Correct(turn) => self.correct(turn),
            Control::Reply(turn) => self.edit_turn(turn),
            Control::History(target) => self.visit_history(target),
            Control::Primary(view) => {
                self.open_evidence(view, true);
            }
            Control::Evidence(view) => self.open_evidence(view, false),
            Control::Fit(view) => {
                self.heights.remove(&view);
                self.open_evidence(view, true);
                self.events
                    .publish(ReviewPaneFocusRequested(ReviewPane::Detail));
            }
            Control::SelectChoice(choice) => self.select_choice(choice),
            control => self.expand(control),
        }
    }

    pub(super) fn edit_answer(&mut self) {
        self.editor_target = EditorTarget::Answer;
        self.editing = self.can_compose();
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

    fn expand(&mut self, control: Control) {
        match control {
            Control::Details(turn)
            | Control::More(turn)
            | Control::References(turn)
            | Control::Supporting(turn) => {
                if let Some(state) = self.turns.get_mut(turn) {
                    state.toggle(control);
                }
            }
            _ => {}
        }
    }

    pub(super) fn visit_opening(&mut self) {
        if self.compose_scope != ComposeScope::Opening {
            self.save_draft();
            self.compose_scope = super::ComposeScope::Opening;
            self.restore_draft();
            self.editing = false;
            self.drag = None;
            self.pointer_view = None;
            self.reveal.set(Some(Reveal::Start));
        }
    }

    fn open_evidence(&mut self, view: EvidenceView, reveal: bool) {
        if let Some(turn) = self.turns.get_mut(view.turn) {
            turn.reference = view.reference;
            self.publish_evidence(view, reveal);
        }
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn cycle_focus(&mut self, event: &ExploreFocusCycle) {
        if !self.can_compose() {
            return;
        }
        if self.general_reply() {
            if self.editing && !event.from_evidence {
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
        let pane = if event.from_evidence {
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

    fn key(&mut self, key: Key) -> Vec<Action> {
        match key {
            Key::Alt('j') => self.resize_by(2),
            Key::Alt('k') => self.resize_by(-2),
            Key::Alt('0') => {
                let view = self.view_id();
                self.heights.remove(&view);
                self.open_evidence(view, true);
            }
            Key::ControlEnter => {
                return self.activate(
                    if self.compose_scope == ComposeScope::Conclusion
                        && self.editor_target == EditorTarget::Implementation
                    {
                        Control::Implement
                    } else {
                        Control::Send
                    },
                );
            }
            key if self.editing => self.edit_current(|editor| editor.input(key)),
            Key::PageDown => self.scroll_by(5),
            Key::PageUp => self.scroll_by(-5),
            key => return self.command(key),
        }
        Vec::new()
    }

    fn command(&mut self, key: Key) -> Vec<Action> {
        if let Some(control) = self.choice_control(key) {
            return self.activate(control);
        }
        match key {
            Key::Enter if self.compose_scope == ComposeScope::Conclusion => {
                self.activate(Control::EditImplementation)
            }
            Key::Enter => self.activate(Control::Reply(self.selected)),
            Key::Up | Key::Char('k') => {
                self.scroll_by(-5);
                Vec::new()
            }
            Key::Down | Key::Char('j') => {
                self.scroll_by(5);
                Vec::new()
            }
            key => self.shortcut(key),
        }
    }

    fn shortcut(&mut self, key: Key) -> Vec<Action> {
        let Key::Char(character) = key else {
            return Vec::new();
        };
        if ('1'..='6').contains(&character) {
            return self.activate(Control::SelectChoice(character as usize - '1' as usize));
        }
        let count = self
            .exploration
            .as_ref()
            .and_then(|exploration| exploration.questions.get(self.selected))
            .map_or(0, |question| question.evidence.len());
        let bindings = [
            ('s', Control::Start),
            ('n', Control::Start),
            ('d', Control::Defer),
            ('[', Control::History(History::Previous)),
            (']', Control::History(History::Next)),
            (
                'b',
                Control::Primary(EvidenceView {
                    turn: self.selected,
                    reference: 0,
                }),
            ),
            ('m', Control::Map),
            ('v', Control::Details(self.selected)),
            ('c', Control::Cancel),
            ('r', Control::Retry),
            ('x', Control::Correct(self.selected)),
            (
                'e',
                Control::Evidence(EvidenceView {
                    turn: self.selected,
                    reference: (self.view_id().reference + 1) % count.max(1),
                }),
            ),
        ];
        bindings
            .into_iter()
            .find(|(binding, _)| *binding == character)
            .map_or_else(Vec::new, |(_, control)| self.activate(control))
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

    fn pointer(&mut self, input: PointerInput) -> Vec<Action> {
        if self.resize_pointer(input) {
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
            self.scroll_by(delta);
            return Vec::new();
        }
        let control = self.layout.borrow().control_at(column, row);
        if matches!(input.kind, PointerInputKind::Click)
            && let Some(control) = control
        {
            self.events
                .publish(ReviewPaneFocusRequested(ReviewPane::Navigation));
            return self.activate(control);
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

struct ExploreKeys;
impl InputMatcher<ExploreComponent, Key> for ExploreKeys {
    type Output = Key;
    fn resolve(&mut self, component: &ExploreComponent, key: &Key) -> InputResolution<Key> {
        if *key == Key::Control('t')
            || (!component.editing && matches!(key, Key::Char('?' | 'q' | 'f' | 't') | Key::Quit))
        {
            InputResolution::NoMatch
        } else {
            InputResolution::Matched(*key)
        }
    }
}
