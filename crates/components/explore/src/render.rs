use super::{
    ComposeScope, Control, EditorTarget, ExploreComponent, Progress,
    flow::{Content, ConversationLayout},
};
use diff_component::{ClippedViewport, DiffComponent};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    widgets::{Block, Borders, Paragraph, Widget, Wrap},
};
use review_explore::Question;
use ui_events::ExploreViewports;
use ui_frame::Frame;
use ui_theme::Palette;

fn format_elapsed(milliseconds: u64) -> String {
    format!("{}.{:01}s", milliseconds / 1000, milliseconds % 1000 / 100)
}

impl ExploreComponent {
    /// Report the evidence windows measured for this Explore pane.
    pub fn viewports(
        &self,
        area: Rect,
        diff: &DiffComponent,
        palette: Palette,
    ) -> ExploreViewports {
        self.conversation_layout(area, diff, palette).viewports()
    }

    /// Measure the same document used for painting and input, independently of Files' width.
    fn conversation_layout(
        &self,
        area: Rect,
        diff: &DiffComponent,
        palette: Palette,
    ) -> ConversationLayout {
        let area = Block::default().borders(Borders::ALL).inner(area);
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
        let block = Frame::Pane { focused }.block(palette, "");
        block.render(area, buffer);
        self.render_conversation(area, buffer, palette, focused, diff);
    }

    fn render_conversation(
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
            self.render_conversation_item(
                super::flow::VisibleContent {
                    item,
                    area: visible,
                    skipped,
                },
                buffer,
                palette,
                focused,
                diff,
            );
        }
    }

    fn render_conversation_item(
        &self,
        visible: super::flow::VisibleContent<'_>,
        buffer: &mut Buffer,
        palette: Palette,
        focused: bool,
        diff: &DiffComponent,
    ) {
        let item = visible.item;
        let area = visible.area;
        let skipped = visible.skipped;
        match &item.content {
            Content::Text(text, _) => {
                Paragraph::new(text.clone())
                    .wrap(Wrap { trim: false })
                    .scroll((skipped, 0))
                    .render(area, buffer);
            }
            Content::EvidenceSplit(list) => {
                self.render_evidence_split(list, visible, buffer, palette, focused, diff);
            }
            Content::Editor(target) => {
                self.render_editor(
                    *target,
                    ClippedViewport::new(area, skipped, item.height),
                    buffer,
                    palette,
                    focused,
                );
            }
            Content::Controls(buttons) => {
                super::controls::Button::render_row(buttons, area, buffer, palette);
            }
        }
    }

    fn render_evidence_split(
        &self,
        list: &super::evidence::EvidenceList,
        visible: super::flow::VisibleContent<'_>,
        buffer: &mut Buffer,
        palette: Palette,
        focused: bool,
        diff: &DiffComponent,
    ) {
        let active = list.view == self.view_id();
        list.render_split(
            visible,
            buffer,
            diff,
            palette,
            !focused && active,
            focused && self.evidence_list_focused && active,
        );
    }

    fn editor_title(&self, target: EditorTarget, editing: bool) -> String {
        let title = match target {
            EditorTarget::Answer if self.selected_choice().is_some() => {
                "Your answer · optional details"
            }
            EditorTarget::Answer => "Your answer",
            EditorTarget::Implementation => "To be implemented",
        };
        let hint = match (editing, target) {
            (false, _) => "",
            (true, EditorTarget::Implementation) => " · Ctrl-Enter Implement · Tab conversation",
            (true, EditorTarget::Answer) => " · Ctrl-Enter Send · Tab conversation",
        };
        format!("{title}{hint}")
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
        let frame = Frame::Pane { focused: editing };
        let block = frame.block(palette, self.editor_title(target, editing));
        let inner = block.inner(area);
        block.render(area, &mut editor);
        let text_editor = match target {
            EditorTarget::Answer => &self.editor,
            EditorTarget::Implementation => &self.conclusion().expect("conclusion editor").editor,
        };
        text_editor.render(inner, &mut editor, palette, true);
        if area.height > 1 {
            text_editor
                .status_border(inner.width, frame.border_style(palette), palette)
                .render(
                    Rect::new(inner.x, area.bottom() - 1, inner.width, 1),
                    &mut editor,
                );
        }
        viewport.draw(&editor, buffer);
    }

    fn transcript(&self, layout: &mut ConversationLayout, diff: &DiffComponent, palette: Palette) {
        if self.compose_scope == ComposeScope::Conclusion {
            self.render_conclusion(layout, palette);
            return;
        }
        if let Some(question) = self.question() {
            let index = self.selected;
            self.preceding_reply(index, layout, palette);
            self.answers(index, question, layout, palette);
            Self::question_sections(question, layout, palette);
            self.evidence_block(index, layout, diff, palette);
        }
        if self.map {
            self.render_map(layout, palette);
        }
    }

    pub(super) fn execution_time(&self, selected_question: Option<&Question>) -> String {
        let Some(exploration) = &self.exploration else {
            return "Agent: —".into();
        };
        let position = exploration
            .conversation
            .iter()
            .position(|turn| match selected_question {
                Some(question) => turn.update.next.as_ref() == Some(question),
                None => self.general_context.as_ref().is_some_and(|request| {
                    turn.update.conclusion.is_some() && &turn.update.request == request
                }),
            });
        let agent = position.and_then(|position| {
            exploration.conversation[..=position]
                .iter()
                .try_fold(0u64, |total, turn| {
                    exploration
                        .agent_elapsed_ms
                        .get(&turn.update.request)
                        .map(|elapsed| total.saturating_add(*elapsed))
                })
        });
        format!(
            "Agent total {}",
            agent.map_or_else(|| "—".into(), format_elapsed)
        )
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
                "You: {}{}{}",
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
        if self.can_cancel(answer)
            && let Some(index) = exploration.answers.iter().position(|a| a.id == answer.id)
        {
            layout.controls([("Cancel answer".into(), Control::CancelAnswer(index))]);
        }
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
        self.answer_marks(answer, layout, palette);
        self.agent_reply(answer, layout, palette);
    }

    /// What the turn after `answer` marked: see [`Self::turn_marks`].
    fn answer_marks(
        &self,
        answer: &review_explore::ReviewerAnswer,
        layout: &mut ConversationLayout,
        palette: Palette,
    ) {
        let turn = self.exploration.as_ref().and_then(|exploration| {
            exploration
                .conversation
                .iter()
                .position(|turn| turn.answer.as_ref() == Some(&answer.id))
        });
        if let Some(turn) = turn {
            self.turn_marks(turn, layout, palette);
        }
    }

    /// What one conversation turn marked reviewed, found not relevant and
    /// reopened: a line to expand into the lines themselves.
    pub(super) fn turn_marks(
        &self,
        index: usize,
        layout: &mut ConversationLayout,
        palette: Palette,
    ) {
        let Some((request, marks)) = self
            .exploration
            .as_ref()
            .and_then(|exploration| exploration.conversation.get(index))
            .and_then(|turn| self.marks.get_key_value(&turn.update.request))
        else {
            return;
        };
        let summary = marks_summary(marks.counts());
        if summary.is_empty() && marks.problem.is_none() {
            return;
        }
        layout.gap();
        if !summary.is_empty() {
            let expanded = self.expanded_marks.contains(request);
            let arrow = if expanded { "▾" } else { "▸" };
            layout.controls([(format!("{arrow} {summary}"), Control::Marks(index))]);
            if expanded {
                Self::marked_lines(marks, layout, palette);
            }
        }
        if let Some(problem) = &marks.problem {
            layout.text(
                format!("Review marks not applied: {problem}"),
                palette.warning,
                None,
            );
        }
    }

    /// The lines a turn's marks changed, one per row.
    fn marked_lines(
        marks: &review_explore::TurnMarks,
        layout: &mut ConversationLayout,
        palette: Palette,
    ) {
        for location in &marks.reviewed {
            layout.text(format!("  ✓ {location}"), palette.dim, None);
        }
        for location in &marks.not_relevant {
            layout.text(format!("  – {location} (not relevant)"), palette.dim, None);
        }
        for reopened in &marks.reopened {
            layout.text(format!("  ↺ {}", reopened.location), palette.dim, None);
        }
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
            layout.controls_right([("Send".into(), Control::Send)]);
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
            layout.controls([("Stop waiting".into(), Control::Cancel)]);
        } else if self.progress == Progress::Retryable {
            layout.controls([("Retry".into(), Control::Retry)]);
        } else if self.progress == Progress::Ready && self.durable.historical {
            layout.controls([("New round".into(), Control::Start)]);
        } else if self.progress == Progress::Ready && self.question().is_none() {
            layout.controls([("Start".into(), Control::Start)]);
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

/// "Marked 4 lines reviewed · 30 lines not relevant · reopened 1 line",
/// naming only what changed.
fn marks_summary(counts: review_explore::MarkCounts) -> String {
    let amount = |lines: u32, files: u32| {
        let plural = |count: u32, one: &str, many: &str| {
            format!("{count} {}", if count == 1 { one } else { many })
        };
        match (lines, files) {
            (0, 0) => None,
            (lines, 0) => Some(plural(lines, "line", "lines")),
            (0, files) => Some(plural(files, "whole file", "whole files")),
            (lines, files) => Some(format!(
                "{} and {}",
                plural(lines, "line", "lines"),
                plural(files, "whole file", "whole files")
            )),
        }
    };
    // Each part as it opens the summary and as it continues it.
    let parts = [
        amount(counts.reviewed_lines, counts.reviewed_files).map(|amount| {
            (
                format!("Marked {amount} reviewed"),
                format!("{amount} reviewed"),
            )
        }),
        amount(counts.not_relevant_lines, counts.not_relevant_files).map(|amount| {
            (
                format!("Marked {amount} not relevant"),
                format!("{amount} not relevant"),
            )
        }),
        amount(counts.reopened_lines, counts.reopened_files)
            .map(|amount| (format!("Reopened {amount}"), format!("reopened {amount}"))),
    ];
    let mut summary = String::new();
    for (opening, continuing) in parts.into_iter().flatten() {
        if summary.is_empty() {
            summary = opening;
        } else {
            summary.push_str(" · ");
            summary.push_str(&continuing);
        }
    }
    summary
}

#[cfg(test)]
#[path = "render.tests.rs"]
mod tests;
