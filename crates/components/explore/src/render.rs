use super::{
    ComposeScope, Control, EditorTarget, ExploreComponent, Progress,
    flow::{Content, ConversationLayout, Window},
};
use diff_component::{ClippedViewport, DiffComponent};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    widgets::{Block, Borders, Paragraph, Widget, Wrap},
};
use review_explore::Question;
use ui_theme::Palette;

impl ExploreComponent {
    /// Measure the same document used for painting and input, independently of Files' width.
    pub fn conversation_layout(
        &self,
        area: Rect,
        diff: &DiffComponent,
        palette: Palette,
    ) -> ConversationLayout {
        let navigation = self.navigation_bar(area);
        let reserved = navigation.height().saturating_add(1);
        let body = Rect::new(
            area.x + 1,
            area.y.saturating_add(reserved),
            area.width.saturating_sub(2),
            area.height.saturating_sub(reserved),
        );
        let mut layout = ConversationLayout::new(body);
        layout.navigation = navigation;
        if self
            .exploration
            .as_ref()
            .is_none_or(|exploration| exploration.conversation.is_empty())
        {
            layout.text(&self.status, palette.text, None);
            self.status_controls(&mut layout);
        } else {
            self.transcript(&mut layout, diff, palette);
        }
        layout.position(self.scroll.get(), self.reveal.take());
        self.scroll.set(layout.scroll);
        self.layout.replace(layout.clone());
        layout
    }

    pub fn render(
        &self,
        area: Rect,
        buffer: &mut Buffer,
        palette: Palette,
        focused: bool,
        diff: &DiffComponent,
    ) {
        let layout = self.conversation_layout(area, diff, palette);
        layout.navigation.render(buffer, palette);
        for item in &layout.items {
            let Some((visible, skipped)) = layout.visible(item) else {
                continue;
            };
            match &item.content {
                Content::Text(text, color, _) => {
                    Paragraph::new(text.as_str())
                        .wrap(Wrap { trim: false })
                        .scroll((skipped, 0))
                        .style(Style::default().fg(*color))
                        .render(visible, buffer);
                }
                Content::Window(view) => {
                    if let Some(viewer) = diff.evidence_view(*view) {
                        viewer.render_embedded(
                            Window::new(*view, visible, skipped, item.height).viewport,
                            buffer,
                            palette,
                            !focused && *view == self.view_id(),
                        );
                    }
                }
                Content::Editor(target) => {
                    self.render_editor(
                        *target,
                        ClippedViewport::new(visible, skipped, item.height),
                        buffer,
                        palette,
                        focused,
                    );
                }
                Content::Resize(_) => Paragraph::new(
                    "──────────────── drag to resize · Alt-j/k · Alt-0 fit ────────────────",
                )
                .style(Style::default().fg(palette.dim))
                .render(visible, buffer),
                Content::Controls(buttons) => {
                    for button in buttons {
                        let width = visible.width.saturating_sub(button.column);
                        Paragraph::new(button.text.as_str())
                            .style(Style::default().fg(palette.focus))
                            .render(
                                Rect::new(
                                    visible.x + button.column.min(visible.width),
                                    visible.y,
                                    width,
                                    1,
                                ),
                                buffer,
                            );
                    }
                }
            }
        }
    }

    fn render_editor(
        &self,
        target: EditorTarget,
        viewport: ClippedViewport,
        buffer: &mut Buffer,
        palette: Palette,
        focused: bool,
    ) {
        let area = viewport.area();
        let mut editor = Buffer::empty(area);
        let editing = self.editing && focused && self.editor_target == target;
        let title = match target {
            EditorTarget::Answer => format!(
                "Your answer{}",
                if self.selected_choice().is_some() {
                    " · optional details"
                } else {
                    ""
                }
            ),
            EditorTarget::Implementation => "To be implemented".into(),
        };
        let hint = if editing {
            if target == EditorTarget::Implementation {
                " · Ctrl-Enter Implement · Tab conversation"
            } else {
                " · Ctrl-Enter Send · Tab conversation"
            }
        } else {
            ""
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {title}{hint} "))
            .border_style(Style::default().fg(if editing { palette.focus } else { palette.dim }));
        let inner = block.inner(area);
        block.render(area, &mut editor);
        match target {
            EditorTarget::Answer => &self.editor,
            EditorTarget::Implementation => &self.conclusion().expect("conclusion editor").editor,
        }
        .render(inner, &mut editor, palette);
        viewport.draw(&editor, buffer);
    }

    fn transcript(&self, layout: &mut ConversationLayout, diff: &DiffComponent, palette: Palette) {
        if self.compose_scope == ComposeScope::Conclusion {
            self.render_conclusion(layout, palette);
            return;
        }
        if self.compose_scope == ComposeScope::Opening {
            self.opening(layout, palette);
        }
        if let Some(question) = self.question() {
            let index = self.selected;
            self.preceding_reply(index, layout, palette);
            self.answers(index, question, layout, palette);
            Self::assessments(question, layout, palette);
            self.turn_controls(index, question, layout, palette);
            self.evidence_block(index, layout, diff, palette);
        }
        if self.map {
            self.render_map(layout, palette);
        }
    }

    fn answers(
        &self,
        index: usize,
        question: &Question,
        layout: &mut ConversationLayout,
        palette: Palette,
    ) {
        let exploration = self.exploration.as_ref().expect("question exploration");
        let answers: Vec<_> = exploration
            .answers
            .iter()
            .filter(|answer| answer.question.as_ref() == Some(question))
            .collect();
        let composing = self.composing_answer(index, !answers.is_empty());
        let heading = layout.height;
        Self::question_heading(index, question, layout, palette);
        if composing {
            self.composer(
                Some(question),
                self.selected_choice().is_some(),
                layout,
                palette,
            );
            if let Some(choice) = &mut layout.choice
                && choice.end.saturating_sub(heading) <= usize::from(layout.area.height)
            {
                choice.start = heading;
            }
        }
        for answer in &answers {
            self.recorded_answer(answer, layout, palette);
        }
        if self.status_turn == Some(index) && !self.status.is_empty() {
            layout.gap();
            layout.text(&self.status, palette.warning, None);
            self.status_controls(layout);
        }
    }

    fn question_heading(
        index: usize,
        question: &Question,
        layout: &mut ConversationLayout,
        palette: Palette,
    ) {
        layout.text(
            format!("Question {} · {}", index + 1, question.text),
            palette.text,
            None,
        );
    }

    pub(super) fn recorded_answer(
        &self,
        answer: &review_explore::ReviewerAnswer,
        layout: &mut ConversationLayout,
        palette: Palette,
    ) {
        let exploration = self.exploration.as_ref().expect("question exploration");
        let selected = answer
            .option
            .as_ref()
            .map_or("", |option| option.text.as_str());
        layout.gap();
        layout.text(
            format!(
                "You{}: {}{}{}",
                if answer.corrects.is_some() {
                    " (correction)"
                } else {
                    ""
                },
                selected,
                if !selected.is_empty() && !answer.text.is_empty() {
                    "\n"
                } else {
                    ""
                },
                answer.text
            ),
            palette.text,
            None,
        );
        for interpretation in exploration
            .interpretations
            .iter()
            .filter(|interpretation| interpretation.answer == answer.id)
        {
            layout.gap();
            layout.text(&interpretation.recap, palette.warning, None);
            for follow_up in &interpretation.follow_ups {
                layout.text(format!("Follow-up: {follow_up}"), palette.warning, None);
            }
        }
        self.agent_reply(answer, layout, palette);
    }

    pub(super) fn composer(
        &self,
        question: Option<&Question>,
        show_options: bool,
        layout: &mut ConversationLayout,
        palette: Palette,
    ) {
        layout.gap();
        if show_options && self.progress.can_submit() {
            for (option, alternative) in
                question.into_iter().flat_map(Question::choices).enumerate()
            {
                let reason = alternative
                    .recommendation
                    .as_ref()
                    .map_or(String::new(), |reason| format!(" — recommended: {reason}"));
                self.answer_item(
                    option,
                    &format!("{}. {}{reason}", option + 1, alternative.text),
                    Control::SelectChoice(option),
                    layout,
                    palette,
                );
            }
        }
        layout.gap();
        layout.answer = Some(layout.height);
        layout.push(Content::Editor(EditorTarget::Answer), 5);
        if self.progress.can_submit() {
            layout.gap();
            let mut controls = vec![("Send".into(), Control::Send)];
            if question.is_some() {
                controls.push(("Defer".into(), Control::Defer));
            }
            layout.controls(controls);
        }
    }

    fn answer_item(
        &self,
        choice: usize,
        text: &str,
        control: Control,
        layout: &mut ConversationLayout,
        palette: Palette,
    ) {
        let selected = self.selected_choice() == Some(choice);
        let top = layout.height;
        layout.text(
            format!("{} {text}", if selected { "›" } else { " " }),
            if selected {
                palette.focus
            } else {
                palette.text
            },
            Some(control),
        );
        if selected {
            layout.choice = Some(top..layout.height);
        }
    }

    pub(super) fn status_controls(&self, layout: &mut ConversationLayout) {
        layout.gap();
        if matches!(self.progress, Progress::Waiting | Progress::Capturing) {
            layout.controls([("Cancel".into(), Control::Cancel)]);
        } else if self.progress == Progress::Retryable {
            layout.controls([("Retry".into(), Control::Retry)]);
        } else if self.progress == Progress::Ready && self.question().is_none() {
            layout.controls([("Start".into(), Control::Start)]);
        }
    }

    fn turn_controls(
        &self,
        index: usize,
        question: &Question,
        layout: &mut ConversationLayout,
        palette: Palette,
    ) {
        let turn = &self.turns[index];
        let mut controls = Vec::new();
        if question.rationale.is_some() || question.visual.is_some() {
            controls.push(("Why this matters".into(), Control::Details(index)));
        } else if question.assessments.is_some() {
            controls.push(("Consequence details".into(), Control::Details(index)));
        }
        controls.push(("Reply".into(), Control::Reply(index)));
        controls.push(("More".into(), Control::More(index)));
        layout.gap();
        layout.controls(controls);
        if turn.details {
            Self::assessment_details(question, layout, palette);
            layout.gap();
            for text in [&question.rationale, &question.visual]
                .into_iter()
                .flatten()
            {
                layout.text(text, palette.text, None);
            }
        }
        if turn.more {
            let mut controls = vec![
                ("Map / follow-ups".into(), Control::Map),
                ("New pass".into(), Control::Start),
            ];
            if self.exploration.as_ref().is_some_and(|exploration| {
                exploration
                    .answers
                    .iter()
                    .any(|answer| answer.question.as_ref() == Some(question))
            }) {
                controls.push(("Correct".into(), Control::Correct(index)));
            }
            layout.gap();
            layout.controls(controls);
            layout.gap();
            layout.text("Up/Down or j/k select an answer; Enter confirms. Tab cycles conversation, evidence, answer. PageUp/Down scroll conversation. Alt-j/k resize evidence; Alt-0 fits it. [ / ] visit questions; e cycles evidence; b returns to primary.",palette.dim,None);
        }
    }

    fn render_map(&self, layout: &mut ConversationLayout, palette: Palette) {
        let exploration = self.exploration.as_ref().expect("question exploration");
        layout.gap();
        self.agenda_map(layout, palette);
        layout.gap();
        for entry in exploration.unmapped() {
            layout.text(
                format!(
                    "Not yet mapped: {} {}",
                    exploration.comparison.files[entry.file].display_path,
                    entry
                        .hunk
                        .map_or_else(String::new, |hunk| format!("hunk {hunk}"))
                ),
                palette.dim,
                None,
            );
        }
        layout.gap();
        for limitation in &exploration.limitations {
            layout.text(format!("Limitation: {limitation}"), palette.warning, None);
        }
        layout.gap();
        for finding in &exploration.findings {
            layout.text(format!("Finding: {finding}"), palette.warning, None);
        }
    }
}
