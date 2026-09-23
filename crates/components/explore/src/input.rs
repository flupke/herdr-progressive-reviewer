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
            Control::Start | Control::PreviousPass | Control::LatestPass => {
                return self.start_or_open(control);
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
            Control::RequireReview(index) => return self.require_review(index),
            control => self.navigate(control),
        }
        Vec::new()
    }

    fn start_or_open(&mut self, control: Control) -> Vec<Action> {
        if matches!(control, Control::Start) {
            self.start()
        } else {
            self.open_saved(control)
        }
    }

    fn require_review(&self, index: usize) -> Vec<Action> {
        if self.completion_done {
            return Vec::new();
        }
        let Some(unit) = self
            .coverage
            .as_ref()
            .and_then(|coverage| coverage.unexplored_exclusions().get(index).cloned())
        else {
            return Vec::new();
        };
        vec![Action::Explore(review_explore::Command::RequireReview(
            Box::new(vec![unit]),
        ))]
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
            Control::PreviewUnexplored => self.open_conclusion_preview(),
            Control::Coverage
            | Control::JevDebug
            | Control::CoverageFile(_)
            | Control::CoverageGap(_)
            | Control::CoverageReturn
            | Control::ExcludedGap(_) => self.navigate_coverage(control),
            Control::Correct(turn) => self.correct(turn),
            Control::Reply(turn) => self.edit_turn(turn),
            Control::History(target) => self.visit_history(target),
            Control::Primary(_) | Control::Evidence(_) | Control::Fit(_) => {
                self.navigate_evidence(control);
            }
            Control::SelectChoice(choice) => self.select_choice(choice),
            control => self.expand(control),
        }
    }

    fn navigate_evidence(&mut self, control: Control) {
        match control {
            Control::Primary(view) => self.open_evidence(view, true),
            Control::Evidence(view) => self.open_evidence(view, false),
            Control::Fit(view) => {
                self.heights.remove(&view);
                self.open_evidence(view, true);
                self.events
                    .publish(ReviewPaneFocusRequested(ReviewPane::Detail));
            }
            _ => {}
        }
    }

    fn navigate_coverage(&mut self, control: Control) {
        match control {
            Control::Coverage => self.toggle_coverage_overview(),
            Control::JevDebug => self.toggle_jev_debug(),
            Control::CoverageFile(index) => self.open_coverage_file(index),
            Control::CoverageGap(index) => self.open_coverage_gap(index, false),
            Control::ExcludedGap(index) => self.open_coverage_gap(index, true),
            Control::CoverageReturn => {
                self.coverage_file = None;
                self.reveal.set(Some(Reveal::Start));
                if self.compose_scope == ComposeScope::Question {
                    self.publish_evidence(self.view_id(), false);
                }
            }
            _ => {}
        }
    }

    fn toggle_coverage_overview(&mut self) {
        if self.coverage_overview {
            self.coverage_overview = false;
            self.coverage_file = None;
            self.scroll
                .set(self.coverage_origin_scroll.take().unwrap_or_default());
            self.reveal.set(None);
            if self.compose_scope == ComposeScope::Question {
                self.publish_evidence(self.view_id(), false);
            }
        } else {
            self.show_coverage_overview();
        }
    }

    fn show_coverage_overview(&mut self) {
        if self.coverage_overview {
            return;
        }
        self.coverage_origin_scroll.set(Some(self.scroll.get()));
        self.coverage_overview = true;
        self.reveal.set(Some(Reveal::Start));
    }

    fn toggle_jev_debug(&mut self) {
        if self.coverage_overview {
            self.jev_debug = !self.jev_debug;
        } else {
            self.show_coverage_overview();
            self.jev_debug = true;
        }
        if self.jev_debug {
            self.reveal.set(Some(Reveal::Jev));
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
            Control::More(turn) | Control::References(turn) | Control::Supporting(turn) => {
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
        let EvidenceView::Question { turn, reference } = view else {
            return;
        };
        if let Some(turn) = self.turns.get_mut(turn) {
            turn.reference = reference;
            self.publish_evidence(view, reveal);
        }
    }

    pub(super) fn open_coverage_file(&mut self, index: usize) {
        let Some(file) = self
            .exploration
            .as_ref()
            .and_then(|pass| pass.comparison.files.get(index))
        else {
            return;
        };
        let path = file.review_path().display();
        self.publish_coverage_view(index, true);
        self.events.publish(ui_events::GuideJumpRequested {
            file_index: index,
            row: None,
            target: review_guide::GuideTarget::File { path },
        });
    }

    fn publish_coverage_view(&mut self, index: usize, reveal: bool) {
        let Some(exploration) = &self.exploration else {
            return;
        };
        let Some(file) = exploration.comparison.files.get(index) else {
            return;
        };
        let comparison = exploration.comparison.clone();
        let path = file.review_path().clone();
        let side = if file.new_path.is_some() {
            review_explore::SourceSide::New
        } else {
            review_explore::SourceSide::Old
        };
        self.show_coverage_overview();
        self.coverage_file = Some(index);
        self.reveal.set(Some(Reveal::CoverageDiff));
        self.events.publish(ui_events::ExploreEvidence {
            comparison,
            evidence: vec![review_explore::EvidenceRef {
                location: review_explore::CodeLocation {
                    path,
                    side,
                    lines: None,
                },
                relationship: "Coverage inspection".into(),
                decision_relevance: String::new(),
            }],
            primary: 1,
            view: Self::coverage_view(),
            reveal,
        });
    }

    fn open_coverage_gap(&mut self, index: usize, excluded: bool) {
        let (Some(coverage), Some(exploration)) = (&self.coverage, &self.exploration) else {
            return;
        };
        let units = if excluded {
            coverage.unexplored_exclusions()
        } else {
            coverage.remaining(self.completion_policy.unwrap_or(self.jev_enabled))
        };
        let Some(unit) = units.get(index) else {
            return;
        };
        let file_index = unit.file_index();
        let following = units
            .iter()
            .enumerate()
            .filter(|(_, candidate)| candidate.file_index() == file_index)
            .map(|(position, _)| position)
            .collect::<Vec<_>>();
        if let Some(position) = following.iter().position(|position| *position == index) {
            self.coverage_next
                .insert(file_index, following[(position + 1) % following.len()]);
        }
        let Some(file) = exploration.comparison.files.get(unit.file_index()) else {
            return;
        };
        let path = file.review_path().display();
        let target = match unit {
            review_explore::CoverageUnit::Lines {
                side, first, end, ..
            } => {
                let range = Some(review_guide::GuideLineRange {
                    first_line: *first,
                    last_line: end - 1,
                });
                review_guide::GuideTarget::Lines {
                    path,
                    old: if *side == review_explore::SourceSide::Old {
                        range.clone()
                    } else {
                        None
                    },
                    new: if *side == review_explore::SourceSide::New {
                        range
                    } else {
                        None
                    },
                }
            }
            review_explore::CoverageUnit::Item { .. } => review_guide::GuideTarget::File { path },
        };
        self.open_coverage_file(file_index);
        self.events.publish(ui_events::GuideJumpRequested {
            file_index: unit.file_index(),
            row: None,
            target,
        });
    }

    pub(super) fn coverage_view() -> EvidenceView {
        EvidenceView::Coverage
    }

    pub(super) fn next_coverage_gap(
        &self,
        units: &[review_explore::CoverageUnit],
        file: usize,
    ) -> Option<usize> {
        let first = units.iter().position(|unit| unit.file_index() == file)?;
        let remembered = self.coverage_next.get(&file).copied().unwrap_or(first);
        units
            .iter()
            .enumerate()
            .find(|(index, unit)| *index >= remembered && unit.file_index() == file)
            .map_or(Some(first), |(index, _)| Some(index))
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
        if self.conclusion_preview.is_some() {
            self.preview_key(key);
            return Vec::new();
        }
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
        if let Key::Alt(_) = key {
            return self.coverage_command(key);
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

    fn coverage_command(&mut self, key: Key) -> Vec<Action> {
        let count = self
            .exploration
            .as_ref()
            .map_or(0, |pass| pass.comparison.files.len());
        let selected = self.coverage_file.unwrap_or(0).min(count.saturating_sub(1));
        match key {
            Key::Alt('o') => {
                if count > 0 {
                    self.activate(Control::CoverageFile(selected))
                } else {
                    Vec::new()
                }
            }
            Key::Alt('n') => {
                let remaining = self.coverage.as_ref().map(|coverage| {
                    coverage.remaining(self.completion_policy.unwrap_or(self.jev_enabled))
                });
                let index = remaining.as_ref().and_then(|units| {
                    let file = self.coverage_file.unwrap_or_else(|| {
                        units
                            .first()
                            .map_or(0, review_explore::CoverageUnit::file_index)
                    });
                    self.next_coverage_gap(units, file)
                });
                index.map_or_else(Vec::new, |index| self.activate(Control::CoverageGap(index)))
            }
            Key::Alt('v') => self.activate(Control::JevDebug),
            Key::Alt('r') => {
                let index = self.coverage.as_ref().and_then(|coverage| {
                    coverage.unexplored_exclusions().iter().position(|unit| {
                        self.coverage_file
                            .is_none_or(|file| unit.file_index() == file)
                    })
                });
                index.map_or_else(Vec::new, |index| {
                    self.activate(Control::RequireReview(index))
                })
            }
            Key::Alt(']' | '[') => {
                if count > 0 {
                    let next = if key == Key::Alt(']') {
                        (selected + 1) % count
                    } else {
                        (selected + count - 1) % count
                    };
                    self.activate(Control::CoverageFile(next))
                } else {
                    Vec::new()
                }
            }
            _ => Vec::new(),
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
            ('g', Control::Coverage),
            ('s', Control::Start),
            ('n', Control::Start),
            ('d', Control::Defer),
            ('[', Control::History(History::Previous)),
            (']', Control::History(History::Next)),
            (
                'b',
                Control::Primary(EvidenceView::Question {
                    turn: self.selected,
                    reference: 0,
                }),
            ),
            ('m', Control::Map),
            ('c', Control::Cancel),
            ('r', Control::Retry),
            ('x', Control::Correct(self.selected)),
            (
                'e',
                Control::Evidence(EvidenceView::Question {
                    turn: self.selected,
                    reference: (self
                        .turns
                        .get(self.selected)
                        .map_or(0, |turn| turn.reference)
                        + 1)
                        % count.max(1),
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
        if self.conclusion_preview.is_some() {
            self.preview_pointer(input);
            return Vec::new();
        }
        self.pointer_conversation(input)
    }

    fn pointer_conversation(&mut self, input: PointerInput) -> Vec<Action> {
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
        if window.view == EvidenceView::Coverage {
            if self.conclusion_preview.is_none()
                && let Some(index) = self.coverage_file
            {
                self.publish_coverage_view(index, false);
            }
        } else {
            self.open_evidence(window.view, false);
        }
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
