use super::{ComposeScope, EditorTarget, ExploreComponent, Progress, TurnView};
use comment_editor::CommentEditor;
use review_explore::{Command, ExplorePage, ExploreViewState, TurnRequest, ViewSave};
use ui_actions::Action;
use ui_events::{
    ExploreCommitted, ExplorePosted, ExploreProgress, ExploreRestored, ExploreStorageFailed,
};

pub(super) struct Durability {
    pub(super) enabled: bool,
    pub(super) error: Option<String>,
    pub(super) historical: bool,
    pub(super) posting: Option<TurnRequest>,
    sequence: u64,
    last: Option<ExploreViewState>,
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

    pub(super) fn begin_round(&mut self) {
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
        self.durable.historical = self
            .exploration
            .as_ref()
            .is_some_and(|round| event.0.is_historical(&round.instance));
    }

    fn page(&self) -> ExplorePage {
        match self.compose_scope {
            ComposeScope::Question => ExplorePage::Question(self.selected),
            ComposeScope::Conclusion => self.general_context.clone().map_or(
                ExplorePage::Question(self.selected),
                ExplorePage::Conclusion,
            ),
        }
    }

    fn saved_view(&self, code: Vec<review_explore::EvidencePosition>) -> ExploreViewState {
        let mut drafts: Vec<_> = self
            .drafts
            .iter()
            .map(|(key, draft)| (key.clone(), draft.saved_state()))
            .collect();
        if self.can_compose() {
            let page = self.page();
            drafts.retain(|(key, _)| key != &page);
            drafts.push((page, self.editor.saved_state()));
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
            heights: self
                .heights
                .iter()
                .map(|(view, height)| {
                    let ui_events::EvidenceView { turn, reference } = view;
                    ((*turn, *reference), *height)
                })
                .collect(),
            front: self.front,
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
        let round = self.exploration.as_ref()?;
        let instance = round.instance.clone();
        let review_unit = round.comparison.checkpoint.review_unit.clone();
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
        let round = match &event.result {
            Ok(Some(round)) => round,
            Ok(None) => {
                self.show_start_screen();
                return;
            }
            Err(error) => {
                self.storage_failed(&ExploreStorageFailed(error.clone()));
                return;
            }
        };
        self.durable.error = None;
        self.durable.historical = event.historical;
        self.durable.sequence = event.view.as_ref().map_or(0, |view| view.sequence);
        self.durable.sequence = self.durable.sequence.max(
            round
                .turns
                .values()
                .filter_map(|turn| turn.editor_sequence)
                .max()
                .unwrap_or(0),
        );
        self.durable.revision = round.revision;
        self.durable.persisted = true;
        self.durable.posting = None;
        self.exploration = Some(round.exploration.clone());
        self.restore_round(round);
        self.turns = round
            .exploration
            .questions
            .iter()
            .map(|_| TurnView::default())
            .collect();
        self.restore_conclusions(round);
        self.drafts.clear();
        self.heights.clear();
        let state = round.restored_view(event.view.as_ref());
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
            round.exploration.comparison.clone(),
        ));
        self.publish_evidence(self.view_id(), false);
        self.events
            .publish(ui_events::ExplorePositionsRestored(state.code.clone()));
        self.exploration
            .as_mut()
            .expect("restored")
            .pause_delivery();
        let (progress, status) = self.recovered(event.progress);
        self.progress = progress;
        self.status = status.into();
        self.status_turn = self.question().map(|_| self.selected);
        self.reveal.set(Some(super::Reveal::RestoreScroll));
        self.durable.last = Some(self.saved_view(state.code));
        if let Some(error) = &event.storage_error {
            self.storage_failed(&ExploreStorageFailed(error.clone()));
        }
    }

    /// What the reviewer can do with a restored round, and the status line saying so.
    fn recovered(&self, progress: ExploreProgress) -> (Progress, &'static str) {
        let (progress, status) = match progress {
            ExploreProgress::Ready => (Progress::Ready, ""),
            ExploreProgress::Interrupted => (
                Progress::Retryable,
                "Interview turn interrupted. Retry keeps the posted answer; reopening has sent nothing.",
            ),
            ExploreProgress::DeliveryUncertain => (
                Progress::Retryable,
                "Previous prompt delivery is uncertain. Retry keeps the posted answer; reopening has sent nothing.",
            ),
        };
        if self.durable.historical {
            (progress, "Earlier round · Reset to start a new one.")
        } else {
            (progress, status)
        }
    }

    fn restore_view(&mut self, state: &ExploreViewState) {
        for (index, (turn, saved)) in self.turns.iter_mut().zip(&state.turns).enumerate() {
            *turn = saved.clone();
            let round = self.exploration.as_ref().expect("restored round");
            turn.choice = turn.choice.min(round.questions[index].alternatives.len());
            turn.reference = turn
                .reference
                .min(round.evidence(index).len().saturating_sub(1));
        }
        for (page, saved) in &state.drafts {
            self.drafts
                .insert(page.clone(), CommentEditor::restore(saved, &self.keymap));
        }
        for (id, text) in &state.tasks {
            if let Some(view) = self.conclusions.get_mut(id) {
                view.editor = CommentEditor::restore(text, &self.keymap);
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
        self.editing = state.editing && self.can_compose() && self.target_editable();
        self.scroll.set(state.scroll);
        self.map = state.map;
        self.front = state.front;
        self.heights = state
            .heights
            .iter()
            .map(|((turn, reference), height)| {
                (
                    ui_events::EvidenceView {
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
            _ => self.restore_first_page(),
        }
    }

    fn restore_first_page(&mut self) {
        if let Some(request) = self
            .exploration
            .as_ref()
            .and_then(|round| {
                round
                    .conversation
                    .iter()
                    .find(|turn| turn.update.next.is_some() || turn.update.conclusion.is_some())
            })
            .filter(|turn| turn.update.conclusion.is_some())
            .map(|turn| &turn.update.request)
            && self.conclusions.contains_key(request)
        {
            self.compose_scope = ComposeScope::Conclusion;
            self.general_context = Some(request.clone());
        } else {
            self.compose_scope = ComposeScope::Question;
            self.selected = 0;
        }
    }

    fn discard_posted_draft(&mut self, answer: &review_explore::ReviewerAnswer) {
        let Some(page) = self
            .exploration
            .as_ref()
            .and_then(|round| round.answer_page(answer))
        else {
            return;
        };
        if self.page() == page {
            if self.editor.text() == answer.text {
                self.editor = CommentEditor::new("", &self.keymap);
            }
        } else if self
            .drafts
            .get(&page)
            .is_some_and(|draft| draft.text() == answer.text)
        {
            self.drafts.remove(&page);
        }
    }

    pub(super) fn posted(&mut self, event: &ExplorePosted) {
        if self.adopt_started(event) {
            return;
        }
        if self
            .exploration
            .as_ref()
            .is_none_or(|round| round.instance != event.request.instance)
        {
            return;
        }
        let current = self
            .durable
            .posting
            .as_ref()
            .is_some_and(|request| request.request == event.request.request);
        match &event.result {
            Ok(round) if round.revision >= self.durable.revision => {
                self.adopt(round);
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
                .send(Err("No Explore round is displayed".into()));
            return;
        };
        if previous.instance != event.round.exploration.instance || self.progress.awaiting_capture()
        {
            let _ = event.response.send(Err(
                "Saved response belongs to another displayed round; reopen its history".into(),
            ));
            return;
        }
        if event.round.revision < self.durable.revision {
            let _ = event.response.send(Ok(false));
            return;
        }
        let count = previous.questions.len();
        let already_visible = previous.conversation == event.round.exploration.conversation;
        self.adopt(&event.round);
        if !already_visible {
            self.update_applied(count, None);
        }
        self.refresh_implementation_delivery(&event.round);
        let _ = event.response.send(Ok(event.applied));
    }

    /// Show a newer saved revision of the displayed round, keeping the
    /// comparison already loaded.
    pub(super) fn adopt(&mut self, round: &review_explore::ExploreRound) {
        let comparison = self
            .exploration
            .as_ref()
            .expect("active")
            .comparison
            .clone();
        let mut exploration = round.exploration.clone();
        exploration.comparison = comparison;
        self.exploration = Some(exploration);
        self.restore_round(round);
        self.durable.revision = round.revision;
        self.durable.persisted = true;
        self.reconcile_history(round);
    }

    fn restore_round(&mut self, round: &review_explore::ExploreRound) {
        self.implementation_requested = !round.implementations.is_empty();
        self.marks.clone_from(&round.marks);
    }

    fn reconcile_history(&mut self, round: &review_explore::ExploreRound) {
        self.turns
            .resize_with(round.exploration.questions.len(), TurnView::default);
        self.reconcile_conclusions(round);
    }

    pub(super) fn storage_failed(&mut self, event: &ExploreStorageFailed) {
        self.durable.error = Some(event.0.clone());
        self.status = format!(
            "Explore storage error: {}. History and current text are retained.",
            event.0
        );
    }
}
