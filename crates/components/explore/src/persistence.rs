use super::{ComposeScope, Control, Draft, EditorTarget, ExploreComponent, Progress, TurnView};
use comment_editor::CommentEditor;
use review_explore::{Command, ExploreDraft, ExplorePage, ExploreViewState, TurnRequest, ViewSave};
use ui_actions::Action;
use ui_events::{
    ExploreCommitted, ExploreCoverageRefresh, ExplorePosted, ExploreRestored, ExploreStorageFailed,
};

pub(super) struct Durability {
    pub(super) enabled: bool,
    pub(super) error: Option<String>,
    pub(super) historical: bool,
    pub(super) posting: Option<TurnRequest>,
    sequence: u64,
    last: Option<ExploreViewState>,
    passes: Vec<String>,
    revision: u64,
    persisted: bool,
    pub(super) focus: ui_events::ReviewPane,
}

impl Default for Durability {
    fn default() -> Self {
        Self {
            enabled: false,
            error: None,
            historical: false,
            posting: None,
            sequence: 0,
            last: None,
            passes: vec![],
            revision: 0,
            persisted: false,
            focus: ui_events::ReviewPane::Navigation,
        }
    }
}

impl Durability {
    pub(super) fn blocked(&self) -> bool {
        self.error.is_some() || self.historical
    }

    pub(super) fn begin_pass(&mut self) {
        self.persisted = false;
        self.revision = 0;
        self.last = None;
        self.posting = None;
        self.historical = false;
        self.sequence = 0;
    }
}

impl ExploreComponent {
    pub(super) fn history_changed(&mut self, event: &ui_events::ExploreHistoryChanged) {
        self.durable.passes.clone_from(&event.0.passes);
        self.durable.historical = self
            .exploration
            .as_ref()
            .is_some_and(|pass| event.0.is_historical(&pass.instance));
    }

    fn page(&self) -> ExplorePage {
        match self.compose_scope {
            ComposeScope::Opening => ExplorePage::Opening,
            ComposeScope::Question => ExplorePage::Question(self.selected),
            ComposeScope::Conclusion => self
                .general_context
                .clone()
                .map_or(ExplorePage::Opening, ExplorePage::Conclusion),
        }
    }

    fn saved_view(&self, code: Vec<review_explore::EvidencePosition>) -> ExploreViewState {
        let mut drafts: Vec<_> = self
            .drafts
            .iter()
            .map(|(key, draft)| {
                (
                    key.clone(),
                    ExploreDraft {
                        editor: draft.editor.saved_state(),
                        correction: draft.correction.clone(),
                    },
                )
            })
            .collect();
        if self.can_compose() {
            let page = self.page();
            drafts.retain(|(key, _)| key != &page);
            drafts.push((
                page,
                ExploreDraft {
                    editor: self.editor.saved_state(),
                    correction: self.correction.clone(),
                },
            ));
        }
        ExploreViewState {
            page: self.page(),
            drafts,
            code,
            turns: self.turns.clone(),
            tasks: self
                .conclusions
                .iter()
                .map(|(id, view)| (id.clone(), view.editor.saved_state()))
                .collect(),
            replies: self
                .conclusions
                .iter()
                .map(|(id, view)| (id.clone(), view.replying))
                .collect(),
            focus: if self.durable.focus == ui_events::ReviewPane::Detail {
                review_explore::EditorFocus::Evidence
            } else {
                match self.editor_target {
                    EditorTarget::Answer => review_explore::EditorFocus::Answer,
                    EditorTarget::Implementation => review_explore::EditorFocus::Implementation,
                }
            },
            editing: self.editing,
            scroll: self.scroll.get(),
            map: self.map,
            coverage_overview: self.coverage_overview,
            coverage_file: self.coverage_file,
            coverage_next: self.coverage_next.clone(),
            jev_debug: self.jev_debug,
            heights: self
                .heights
                .iter()
                .filter_map(|(view, height)| match view {
                    ui_events::EvidenceView::Question { turn, reference } => {
                        Some(((*turn, *reference), *height))
                    }
                    ui_events::EvidenceView::Coverage => None,
                })
                .collect(),
        }
    }

    /// Called after input/events, never from rendering. Only small view state is autosaved.
    pub(super) fn autosave(&mut self, event: &ui_events::ExploreAutosave) -> Option<Action> {
        if let Some(focus) = event.focus {
            self.durable.focus = focus;
        }
        if !self.durable.enabled
            || !self.durable.persisted
            || self.durable.error.is_some()
            || self.progress.awaiting_capture()
        {
            return None;
        }
        let pass = self.exploration.as_ref()?;
        let instance = pass.instance.clone();
        let review_unit = pass.comparison.checkpoint.review_unit.clone();
        let state = self.saved_view(event.positions.clone());
        if self.durable.last.as_ref() == Some(&state) {
            return None;
        }
        self.durable.last = Some(state.clone());
        self.durable.sequence += 1;
        Some(Action::Explore(Command::SaveView(Box::new(ViewSave {
            review_unit,
            instance,
            sequence: self.durable.sequence,
            state,
        }))))
    }

    pub(super) fn restored(&mut self, event: &ExploreRestored) {
        self.durable.enabled = true;
        self.durable.passes.clone_from(&event.passes);
        let pass = match &event.result {
            Ok(Some(pass)) => pass,
            Ok(None) => {
                let mode = self.mode;
                *self = Self::new(self.events.clone());
                self.mode = mode;
                self.durable.enabled = true;
                return;
            }
            Err(error) => {
                self.storage_failed(&ExploreStorageFailed(error.clone()));
                return;
            }
        };
        self.durable.error = None;
        self.durable.historical = event.historical;
        self.durable.passes.clone_from(&event.passes);
        self.durable.sequence = event.view.as_ref().map_or(0, |view| view.sequence);
        self.durable.sequence = self.durable.sequence.max(
            pass.turns
                .values()
                .filter_map(|turn| turn.editor_sequence)
                .max()
                .unwrap_or(0),
        );
        self.durable.revision = pass.revision;
        self.durable.persisted = true;
        self.durable.posting = None;
        self.exploration = Some(pass.exploration.clone());
        self.restore_coverage(pass);
        self.turns = pass
            .exploration
            .questions
            .iter()
            .map(|_| TurnView::default())
            .collect();
        self.restore_conclusions(pass);
        self.drafts.clear();
        self.heights.clear();
        let state = pass.restored_view(event.view.as_ref());
        self.restore_view(&state);
        self.durable.focus = if state.focus == review_explore::EditorFocus::Evidence {
            ui_events::ReviewPane::Detail
        } else {
            ui_events::ReviewPane::Navigation
        };
        if self.mode == ui_events::ReviewNavigation::Explore {
            self.events
                .publish(ui_events::ReviewPaneFocusRequested(self.durable.focus));
        }
        self.events.publish(ui_events::ExploreComparisonAccepted(
            pass.exploration.comparison.clone(),
        ));
        self.publish_evidence(self.view_id(), false);
        if self.coverage_overview
            && let Some(index) = self.coverage_file
        {
            self.open_coverage_file(index);
        }
        self.events
            .publish(ui_events::ExplorePositionsRestored(state.code.clone()));
        self.exploration
            .as_mut()
            .expect("restored")
            .pause_delivery();
        self.progress = if pass.exploration.retry_request().is_some() {
            Progress::Retryable
        } else {
            Progress::Ready
        };
        self.status = self.recovery_status(pass);
        self.status_turn = self.question().map(|_| self.selected);
        self.reveal.set(Some(super::Reveal::RestoreScroll));
        self.durable.last = Some(self.saved_view(state.code));
        if let Some(error) = &event.storage_error {
            self.storage_failed(&ExploreStorageFailed(error.clone()));
        }
    }

    fn recovery_status(&self, pass: &review_explore::ExplorePass) -> String {
        if self.durable.historical {
            "Earlier pass · open the latest pass to continue.".into()
        } else if self.progress == Progress::Retryable {
            let uncertain = pass.exploration.retry_request().is_some_and(|request| {
                pass.turns.get(&request.request).is_some_and(|delivery| {
                    matches!(
                        delivery.state,
                        review_explore::DispatchState::Attempting
                            | review_explore::DispatchState::Unknown
                    )
                })
            });
            if uncertain {
                "Previous prompt delivery is uncertain. Retry keeps the posted answer; reopening has sent nothing.".into()
            } else {
                "Interview turn interrupted. Retry keeps the posted answer; reopening has sent nothing.".into()
            }
        } else {
            String::new()
        }
    }

    fn restore_view(&mut self, state: &ExploreViewState) {
        for (index, (turn, saved)) in self.turns.iter_mut().zip(&state.turns).enumerate() {
            *turn = saved.clone();
            let pass = self.exploration.as_ref().expect("restored pass");
            turn.choice = turn.choice.min(pass.questions[index].alternatives.len());
            turn.reference = turn
                .reference
                .min(pass.evidence(index).len().saturating_sub(1));
        }
        for (page, saved) in &state.drafts {
            self.drafts.insert(
                page.clone(),
                Draft {
                    editor: CommentEditor::restore(&saved.editor),
                    correction: saved.correction.clone(),
                },
            );
        }
        for (id, text) in &state.tasks {
            if let Some(view) = self.conclusions.get_mut(id) {
                view.editor = CommentEditor::restore(text);
            }
        }
        for (id, replying) in &state.replies {
            if let Some(view) = self.conclusions.get_mut(id) {
                view.replying = *replying;
            }
        }
        self.restore_page(&state.page);
        self.editor_target = match state.focus {
            review_explore::EditorFocus::Implementation
                if self.compose_scope == ComposeScope::Conclusion =>
            {
                EditorTarget::Implementation
            }
            _ => EditorTarget::Answer,
        };
        self.restore_draft();
        self.editing = state.editing && self.can_compose();
        self.scroll.set(state.scroll);
        self.map = state.map;
        self.coverage_overview = state.coverage_overview;
        self.coverage_origin_scroll.set(None);
        self.coverage_file = state.coverage_file;
        self.coverage_next.clone_from(&state.coverage_next);
        self.jev_debug = state.jev_debug;
        self.heights = state
            .heights
            .iter()
            .map(|((turn, reference), height)| {
                (
                    ui_events::EvidenceView::Question {
                        turn: *turn,
                        reference: *reference,
                    },
                    *height,
                )
            })
            .collect();
    }

    fn restore_page(&mut self, page: &ExplorePage) {
        match page {
            ExplorePage::Question(index) if *index < self.turns.len() => {
                self.compose_scope = ComposeScope::Question;
                self.selected = *index;
            }
            ExplorePage::Conclusion(id) if self.conclusions.contains_key(id) => {
                self.compose_scope = ComposeScope::Conclusion;
                self.general_context = Some(id.clone());
            }
            _ => self.compose_scope = ComposeScope::Opening,
        }
    }

    fn discard_posted_draft(&mut self, answer: &review_explore::ReviewerAnswer) {
        let Some(page) = self
            .exploration
            .as_ref()
            .and_then(|pass| pass.answer_page(answer))
        else {
            return;
        };
        if self.page() == page {
            if self.editor.text() == answer.text && self.correction == answer.corrects {
                self.editor = CommentEditor::new("", self.editor.keymap());
                self.correction = None;
            }
        } else if self.drafts.get(&page).is_some_and(|draft| {
            draft.editor.text() == answer.text && draft.correction == answer.corrects
        }) {
            self.drafts.remove(&page);
        }
    }

    pub(super) fn posted(&mut self, event: &ExplorePosted) {
        if self
            .exploration
            .as_ref()
            .is_none_or(|pass| pass.instance != event.request.instance)
        {
            return;
        }
        let current = self
            .durable
            .posting
            .as_ref()
            .is_some_and(|request| request.request == event.request.request);
        match &event.result {
            Ok(pass) if pass.revision >= self.durable.revision => {
                let comparison = self
                    .exploration
                    .as_ref()
                    .expect("active")
                    .comparison
                    .clone();
                self.exploration = Some(pass.exploration.clone());
                self.restore_coverage(pass);
                self.exploration.as_mut().expect("active").comparison = comparison;
                self.durable.revision = pass.revision;
                self.durable.persisted = true;
                self.reconcile_history(pass);
                if current && self.progress == Progress::Waiting {
                    if let Some(answer) = &event.request.answer {
                        self.discard_posted_draft(answer);
                    }
                    self.status = "Waiting for the implementation agent…".into();
                } else {
                    // Retain the durable contribution without reviving a cancelled or
                    // replaced local post. The next matching acknowledgement owns progress.
                    self.exploration.as_mut().expect("active").pause_delivery();
                }
            }
            Err(error) if current => {
                self.status = format!("Answer was not posted: {error}");
                self.progress = Progress::Ready;
            }
            _ => {}
        }
        if current {
            self.durable.posting = None;
        }
    }

    pub(super) fn committed(&mut self, event: &ExploreCommitted) {
        let Some(previous) = &self.exploration else {
            let _ = event
                .response
                .send(Err("No Explore pass is displayed".into()));
            return;
        };
        if previous.instance != event.pass.exploration.instance || self.progress.awaiting_capture()
        {
            let _ = event.response.send(Err(
                "Saved response belongs to another displayed pass; reopen its history".into(),
            ));
            return;
        }
        if event.pass.revision < self.durable.revision {
            let _ = event.response.send(Ok(false));
            return;
        }
        let count = previous.questions.len();
        let already_visible = previous.conversation == event.pass.exploration.conversation;
        let comparison = previous.comparison.clone();
        self.exploration = Some(event.pass.exploration.clone());
        self.restore_coverage(&event.pass);
        self.exploration.as_mut().expect("active").comparison = comparison;
        self.durable.revision = event.pass.revision;
        self.reconcile_history(&event.pass);
        if !already_visible {
            self.update_applied(count, None);
        }
        self.refresh_implementation_delivery(&event.pass);
        let _ = event.response.send(Ok(event.applied));
    }

    fn restore_coverage(&mut self, pass: &review_explore::ExplorePass) {
        self.coverage = Some(pass.coverage.clone());
        self.jev_progress_expiry.observe(&pass.coverage);
        self.conclusion_unexplored = pass.completion.as_ref().and_then(|completion| {
            completion
                .unexplored
                .clone()
                .map(|unexplored| (completion.request.clone(), unexplored))
        });
        self.conclusion_preview = None;
        self.completion_done = pass
            .completion
            .as_ref()
            .is_some_and(|completion| completion.completed);
        self.completion_policy = pass
            .completion
            .as_ref()
            .filter(|completion| completion.completed)
            .map(|completion| completion.exclusions_enabled);
        self.coverage_dirty = true;
    }

    pub(super) fn refresh_coverage(&mut self, _event: &ExploreCoverageRefresh) {
        if !self.coverage_dirty {
            return;
        }
        let (Some(pass), Some(coverage)) = (&self.exploration, &self.coverage) else {
            return;
        };
        self.coverage_cache.refresh(
            &pass.instance,
            coverage,
            &pass.comparison,
            self.completion_policy.unwrap_or(self.jev_enabled),
        );
        self.coverage_dirty = false;
    }

    fn reconcile_history(&mut self, pass: &review_explore::ExplorePass) {
        self.turns
            .resize_with(pass.exploration.questions.len(), TurnView::default);
        self.reconcile_conclusions(pass);
    }

    pub(super) fn storage_failed(&mut self, event: &ExploreStorageFailed) {
        self.durable.error = Some(event.0.clone());
        self.status = format!(
            "Explore storage error: {}. History and current text are retained.",
            event.0
        );
    }

    pub(super) fn saved_controls(&self) -> Vec<(String, Option<Control>)> {
        let mut controls = Vec::new();
        if !self.progress.can_submit() || self.implementation_in_progress() {
            return controls;
        }
        if self.durable.passes.len() > 1 {
            controls.push(("[Previous pass]".into(), Some(Control::PreviousPass)));
            if self.durable.historical {
                controls.push(("[Latest pass]".into(), Some(Control::LatestPass)));
            }
        }
        controls
    }

    pub(super) fn open_saved(&mut self, control: Control) -> Vec<Action> {
        if !self.progress.can_submit() || self.implementation_in_progress() {
            return vec![];
        }
        let command = match control {
            Control::PreviousPass => {
                let current = self.exploration.as_ref().map(|pass| &pass.instance);
                let index = self
                    .durable
                    .passes
                    .iter()
                    .position(|id| Some(id) == current)
                    .unwrap_or(self.durable.passes.len().saturating_sub(1));
                self.durable
                    .passes
                    .get(index.saturating_sub(1))
                    .cloned()
                    .map(Command::OpenPass)
            }
            Control::LatestPass => self.durable.passes.last().cloned().map(Command::OpenPass),
            _ => None,
        };
        command
            .map(|command| vec![Action::Explore(command)])
            .unwrap_or_default()
    }
}
