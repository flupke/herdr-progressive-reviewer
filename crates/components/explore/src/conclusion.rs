use super::{
    ComposeScope, Control, EditorTarget, ExploreComponent, Reveal,
    flow::{Content, ConversationLayout},
};
use comment_editor::CommentEditor;
use ratatui::widgets::{Paragraph, Wrap};
use ui_actions::Action;
use ui_events::{
    ExploreImplementationFinished, ReviewNavigation, ReviewPane, ReviewPaneFocusRequested,
};
use ui_theme::Palette;

enum Delivery {
    Ready,
    Pending(String),
    Cancelling(String),
    Sent,
    Failed(String),
    Paused(review_explore::ImplementationRequest),
    Unknown(review_explore::ImplementationRequest),
}

impl Delivery {
    fn is_pending(&self) -> bool {
        matches!(self, Self::Pending(_) | Self::Cancelling(_))
    }

    fn can_submit(&self) -> bool {
        !self.is_pending() && !matches!(self, Self::Sent | Self::Unknown(_))
    }
}

pub(super) struct ConclusionView {
    request: String,
    content: review_explore::Conclusion,
    pub(super) editor: CommentEditor,
    pub(super) replying: bool,
    delivery: Delivery,
    attempt: Option<String>,
}

impl ExploreComponent {
    fn marking_ready(&self) -> bool {
        !self.durable.enabled || self.completion_done
    }

    pub(super) fn implementation_in_progress(&self) -> bool {
        self.conclusions
            .values()
            .any(|view| view.delivery.is_pending())
    }

    pub(super) fn accept_conclusion(&mut self) {
        let exploration = self.exploration.as_ref().expect("active exploration");
        let request = exploration
            .conversation
            .last()
            .expect("accepted conclusion")
            .update
            .request
            .clone();
        let content = exploration.conclusion.clone().expect("accepted conclusion");
        if !self.durable.enabled
            && self.conclusion_unexplored.is_none()
            && let Some(coverage) = &self.coverage
        {
            let exclusions_enabled = self.completion_policy.unwrap_or(self.jev_enabled);
            self.conclusion_unexplored = Some((
                request.clone(),
                review_explore::UnexploredAtConclusion {
                    required: coverage.remaining(exclusions_enabled),
                    jev_excluded: if exclusions_enabled {
                        coverage.unexplored_exclusions()
                    } else {
                        Vec::new()
                    },
                },
            ));
        }
        self.conclusions.insert(
            request.clone(),
            ConclusionView {
                request: request.clone(),
                editor: CommentEditor::new(&content.to_be_implemented, self.editor.keymap()),
                content,
                replying: false,
                delivery: Delivery::Ready,
                attempt: None,
            },
        );
        self.visit_conclusion_at(request);
        if self.mode == ReviewNavigation::Explore {
            self.events
                .publish(ReviewPaneFocusRequested(ReviewPane::Navigation));
        }
    }

    pub(super) fn conclusion(&self) -> Option<&ConclusionView> {
        self.conclusions.get(self.general_context.as_ref()?)
    }

    pub(super) fn conclusion_mut(&mut self) -> Option<&mut ConclusionView> {
        self.conclusions.get_mut(self.general_context.as_ref()?)
    }

    fn active_conclusion_selected(&self) -> bool {
        self.exploration
            .as_ref()
            .and_then(|exploration| exploration.conclusion_request())
            .is_some_and(|request| self.general_context.as_deref() == Some(request))
    }

    pub(super) fn visit_conclusion_at(&mut self, request: String) {
        if !self.conclusions.contains_key(&request) {
            return;
        }
        self.save_draft();
        self.conclusion_preview = None;
        self.compose_scope = ComposeScope::Conclusion;
        self.general_context = Some(request);
        self.restore_draft();
        self.editing = false;
        self.editor_target = if self.conclusion().is_some_and(|view| view.replying) {
            EditorTarget::Answer
        } else {
            EditorTarget::Implementation
        };
        self.drag = None;
        self.pointer_view = None;
        self.reveal.set(Some(Reveal::Start));
    }

    pub(super) fn edit_implementation(&mut self) {
        if self.compose_scope == ComposeScope::Conclusion
            && self
                .conclusion()
                .is_some_and(|view| !view.delivery.is_pending())
        {
            self.editor_target = EditorTarget::Implementation;
            self.editing = true;
            self.reveal
                .set(Some(Reveal::Editor(EditorTarget::Implementation)));
        }
    }

    pub(super) fn implementation_changed(&mut self) {
        if self.editor_target == EditorTarget::Implementation
            && let Some(view) = self.conclusion_mut()
            && !view.delivery.is_pending()
            && !matches!(view.delivery, Delivery::Paused(_) | Delivery::Unknown(_))
        {
            view.delivery = Delivery::Ready;
        }
    }

    pub(super) fn implement(&mut self) -> Vec<Action> {
        if self.compose_scope != ComposeScope::Conclusion
            || !self.progress.can_submit()
            || self.durable.blocked()
            || !self.marking_ready()
            || !self.active_conclusion_selected()
        {
            return vec![];
        }
        let Some(view) = self.conclusion() else {
            return vec![];
        };
        if !view.delivery.can_submit() {
            return vec![];
        }
        let exploration = self.exploration.as_ref().expect("conclusion exploration");
        let result = if let Delivery::Paused(request) = &view.delivery {
            Ok(request.clone())
        } else {
            exploration.implementation(view.editor.text())
        };
        let view = self.conclusion_mut().expect("selected conclusion");
        match result {
            Ok(request) => {
                view.delivery = Delivery::Pending(request.delivery.clone());
                view.attempt = None;
                self.editing = false;
                vec![Action::Explore(review_explore::Command::Implement(request))]
            }
            Err(error) => {
                view.delivery = Delivery::Failed(error.to_string());
                vec![]
            }
        }
    }

    pub(super) fn cancel_implementation(&mut self) -> Vec<Action> {
        if let Some(view) = self.conclusion_mut()
            && let Delivery::Pending(id) = &view.delivery
        {
            view.delivery = Delivery::Cancelling(id.clone());
            return vec![Action::Explore(
                review_explore::Command::CancelImplementation,
            )];
        }
        vec![]
    }

    pub(super) fn implementation_finished(&mut self, event: &ExploreImplementationFinished) {
        if self
            .exploration
            .as_ref()
            .is_none_or(|exploration| exploration.instance != event.request.instance)
        {
            return;
        }
        if let Some(view) = self.conclusions.get_mut(&event.request.conclusion)
            && matches!(&view.delivery, Delivery::Pending(id) | Delivery::Cancelling(id) if id == &event.request.delivery)
            && view.attempt == event.attempt
        {
            view.delivery = Delivery::from_state(&event.request, &event.state);
        }
    }

    pub(super) fn render_conclusion(&self, layout: &mut ConversationLayout, palette: Palette) {
        let Some(view) = self.conclusion() else {
            return;
        };
        layout.text("Summary", palette.focus, None);
        layout.text(&view.content.summary, palette.text, None);
        layout.gap();
        let rows = Paragraph::new(view.editor.text())
            .wrap(Wrap { trim: false })
            .line_count(layout.area.width.saturating_sub(2).max(1));
        let height = u16::try_from(rows.saturating_add(4))
            .unwrap_or(u16::MAX)
            .clamp(6, layout.area.height.saturating_sub(6).max(6));
        layout.push(Content::Editor(EditorTarget::Implementation), height);
        layout.gap();
        self.implementation_controls(layout, palette);
        if !view.content.future_work.is_empty() {
            layout.gap();
            layout.text("Future work", palette.focus, None);
            layout.text(&view.content.future_work, palette.text, None);
        }
        layout.gap();
        if let Some((request, unexplored)) = &self.conclusion_unexplored
            && request == &view.request
        {
            let count = unexplored.required.len() + unexplored.jev_excluded.len();
            layout.text(
                format!("{count} unexplored changed regions at this checkpoint"),
                if count > 0 {
                    palette.warning
                } else {
                    palette.dim
                },
                None,
            );
            if count > 0 {
                layout.controls([("Preview unexplored code".into(), Control::PreviewUnexplored)]);
            }
            layout.gap();
        }
        layout.text(
            "Further human Files inspection is required.",
            palette.dim,
            None,
        );
        layout.gap();
        layout.controls([
            ("Reply".into(), Control::GeneralReply),
            ("New pass".into(), Control::Start),
        ]);
        let exploration = self.exploration.as_ref().expect("conclusion exploration");
        for answer in exploration
            .answers
            .iter()
            .filter(|answer| answer.in_reply_to == view.request && answer.question.is_none())
        {
            self.recorded_answer(answer, layout, palette);
        }
        if !self.status.is_empty() {
            layout.gap();
            layout.text(&self.status, palette.text, None);
            self.status_controls(layout);
        }
        if view.replying {
            self.composer(None, false, layout, palette);
        }
    }

    fn implementation_controls(&self, layout: &mut ConversationLayout, palette: Palette) {
        let view = self.conclusion().expect("conclusion view");
        if !self.marking_ready() {
            layout.text(
                "Explore file marking is pending; restore or retry the accepted conclusion before implementing.",
                palette.warning,
                None,
            );
            return;
        }
        if !self.active_conclusion_selected() || self.durable.historical {
            layout.text(
                "The interview has continued since this conclusion.",
                palette.dim,
                None,
            );
            return;
        }
        match &view.delivery {
            Delivery::Cancelling(_) => layout.text(
                "Cancelling queued implementation request…",
                palette.text,
                None,
            ),
            Delivery::Pending(_) => {
                layout.text("Sending implementation request…", palette.text, None);
                layout.controls([(
                    "Cancel implementation".into(),
                    Control::CancelImplementation,
                )]);
            }
            Delivery::Paused(request) => {
                layout.text(
                    "Saved implementation request is paused; it has not been sent.",
                    palette.text,
                    None,
                );
                if view.editor.text() != request.text {
                    layout.text("Send saved uses the previously authorized text. New request uses the current editor text.", palette.text, None);
                }
                layout.controls([
                    (
                        "Send saved implementation request".into(),
                        Control::Implement,
                    ),
                    (
                        "New implementation request".into(),
                        Control::NewImplementation,
                    ),
                ]);
            }
            Delivery::Unknown(request) => {
                layout.text(
                    format!("Implementation request {}", request.delivery),
                    palette.dim,
                    None,
                );
                layout.text("Delivery outcome unknown. Check the original agent conversation before deliberately sending a new request.", palette.warning, None);
                layout.controls([(
                    "New implementation request".into(),
                    Control::NewImplementation,
                )]);
            }
            Delivery::Sent => layout.text(
                "Implementation request sent to the agent.",
                palette.text,
                None,
            ),
            state => self.implementation_ready_controls(state, layout, palette),
        }
    }

    fn implementation_ready_controls(
        &self,
        state: &Delivery,
        layout: &mut ConversationLayout,
        palette: Palette,
    ) {
        if let Delivery::Failed(error) = state {
            layout.text(error, palette.warning, None);
        }
        if self.progress.can_submit()
            && !self.durable.blocked()
            && self
                .conclusion()
                .is_some_and(|view| !view.editor.text().trim().is_empty())
        {
            layout.controls([("Implement".into(), Control::Implement)]);
        }
    }
}

impl ExploreComponent {
    pub(super) fn refresh_implementation_delivery(&mut self, pass: &review_explore::ExplorePass) {
        for (id, view) in &mut self.conclusions {
            let Some(record) = pass
                .implementations
                .values()
                .filter(|record| &record.request.conclusion == id)
                .max_by_key(|record| record.authorized_at)
            else {
                continue;
            };
            let pending_here = matches!(&view.delivery, Delivery::Pending(id) | Delivery::Cancelling(id) if id == &record.request.delivery);
            if pending_here
                && matches!(
                    record.state,
                    review_explore::DispatchState::Queued
                        | review_explore::DispatchState::Attempting
                )
            {
                continue;
            }
            view.delivery = Delivery::restored(record);
            view.attempt = Some(record.attempt.clone());
        }
    }

    pub(super) fn new_implementation(&mut self) {
        if let Some(view) = self.conclusion_mut()
            && !view.delivery.is_pending()
        {
            view.delivery = Delivery::Ready;
        }
    }

    pub(super) fn restore_conclusions(&mut self, pass: &review_explore::ExplorePass) {
        self.conclusions.clear();
        self.reconcile_conclusions(pass);
    }

    pub(super) fn reconcile_conclusions(&mut self, pass: &review_explore::ExplorePass) {
        for turn in &pass.exploration.conversation {
            let Some(content) = &turn.update.conclusion else {
                continue;
            };
            if self.conclusions.contains_key(&turn.update.request) {
                continue;
            }
            let delivery = pass
                .implementations
                .values()
                .filter(|record| record.request.conclusion == turn.update.request)
                .max_by_key(|record| record.authorized_at)
                .map_or(Delivery::Ready, Delivery::restored);
            self.conclusions.insert(
                turn.update.request.clone(),
                ConclusionView {
                    request: turn.update.request.clone(),
                    content: content.clone(),
                    editor: CommentEditor::new(&content.to_be_implemented, self.editor.keymap()),
                    replying: false,
                    delivery,
                    attempt: None,
                },
            );
        }
    }

    pub(super) fn implementation_saved(&mut self, event: &ui_events::ExploreImplementationSaved) {
        let record = &event.0;
        if self
            .exploration
            .as_ref()
            .is_none_or(|pass| pass.instance != record.request.instance)
        {
            return;
        }
        if let Some(view) = self.conclusions.get_mut(&record.request.conclusion)
            && matches!(&view.delivery, Delivery::Pending(id) | Delivery::Cancelling(id) if id == &record.request.delivery)
        {
            view.attempt = Some(record.attempt.clone());
            // A queued result is not a recovered paused request in this running process.
            if record.state != review_explore::DispatchState::Queued {
                view.delivery = Delivery::restored(record);
            }
        }
    }
}

impl Delivery {
    fn restored(record: &review_explore::ImplementationDelivery) -> Self {
        Self::from_state(&record.request, &record.state)
    }

    fn from_state(
        request: &review_explore::ImplementationRequest,
        state: &review_explore::DispatchState,
    ) -> Self {
        use review_explore::DispatchState;
        match state.recovered() {
            DispatchState::Queued => Self::Paused(request.clone()),
            DispatchState::Attempting | DispatchState::Unknown => Self::Unknown(request.clone()),
            DispatchState::Delivered => Self::Sent,
            DispatchState::Cancelled => {
                Self::Failed("Implementation request cancelled before sending.".into())
            }
            DispatchState::NotSent(error) => Self::Failed(error),
        }
    }
}
