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

fn format_elapsed(milliseconds: u64) -> String {
    format!("{}.{:01}s", milliseconds / 1000, milliseconds % 1000 / 100)
}

impl ExploreComponent {
    /// Measure the same document used for painting and input, independently of Files' width.
    pub fn conversation_layout(
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
        let content = if self.coverage_overview {
            Block::default().borders(Borders::ALL).inner(body)
        } else {
            body
        };
        let mut layout = ConversationLayout::new(content);
        layout.navigation = navigation;
        layout.frame = self.coverage_overview.then_some(body);
        if self.coverage_overview {
            self.render_coverage_overview(&mut layout, diff, palette);
        } else if self
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

    fn render_coverage_overview(
        &self,
        layout: &mut ConversationLayout,
        diff: &DiffComponent,
        palette: Palette,
    ) {
        let Some(coverage) = self.coverage_cache.get() else {
            return;
        };
        let summary = &coverage.summary;
        let lines = coverage.lines;
        if summary.complete && lines.total > 0 {
            layout.text(
                format!(
                    "{} of required changed lines answered ({} of {})",
                    Self::changed_line_percent(lines),
                    lines.explored,
                    lines.total
                ),
                palette.text,
                None,
            );
        } else if summary.complete {
            layout.text("No required added or deleted lines", palette.text, None);
        } else {
            layout.text("Changed-line count unavailable", palette.warning, None);
        }
        layout.text(
            "Counts answered added/deleted lines divided by lines still requiring review after Jev filtering; context and metadata omitted. Filtering is not human review.",
            palette.dim,
            None,
        );
        layout.text(
            format!(
                "To finish: {} required lines or metadata changes remain · {} Jev-excluded changes",
                summary.remaining, summary.excluded_unexplored
            ),
            palette.text,
            None,
        );
        layout.text(
            "Show file diff displays that file below and selects it in Files.",
            palette.dim,
            None,
        );
        layout.text(
            "Keys: g toggle · Alt-[ / Alt-] files · Alt-o open · Alt-n next gap · Alt-v Jev · Alt-r require review",
            palette.dim,
            None,
        );
        for limitation in &summary.limitations {
            layout.text(format!("Inventory: {limitation}"), palette.warning, None);
        }
        self.render_coverage_files(layout, &coverage.files, &coverage.remaining, palette);
        self.render_jev_debug(layout, palette);
        self.render_coverage_diff(layout, diff, palette);
        layout.gap();
    }

    fn coverage_group(file: &review_explore::FileCoverage) -> u8 {
        let summary = &file.summary;
        if !summary.complete {
            4
        } else if summary.required == 0 && summary.total > 0 {
            3
        } else if summary.remaining == 0 {
            0
        } else if summary.explored_required > 0 {
            1
        } else {
            2
        }
    }

    fn render_coverage_files(
        &self,
        layout: &mut ConversationLayout,
        files: &[super::coverage::CachedFileCoverage],
        remaining: &[review_explore::CoverageUnit],
        palette: Palette,
    ) {
        for (heading, predicate) in [
            ("All required changes answered", 0_u8),
            ("Some required changes answered", 1),
            ("Needs answers", 2),
            ("Jev-excluded only", 3),
            ("Inventory incomplete", 4),
        ] {
            let group: Vec<_> = files
                .iter()
                .filter(|file| Self::coverage_group(&file.coverage) == predicate)
                .collect();
            if group.is_empty() {
                continue;
            }
            layout.gap();
            layout.text(heading, palette.text, None);
            for file in group {
                self.render_coverage_file(layout, file, remaining, palette);
            }
        }
    }

    fn render_coverage_file(
        &self,
        layout: &mut ConversationLayout,
        cached: &super::coverage::CachedFileCoverage,
        remaining: &[review_explore::CoverageUnit],
        palette: Palette,
    ) {
        let file = &cached.coverage;
        let lines = cached.lines;
        let progress = if !file.summary.complete {
            "changed-line count unavailable".into()
        } else if lines.total == 0 {
            "no required changed text lines".into()
        } else {
            format!(
                "{} of required changed lines answered ({} of {})",
                Self::changed_line_percent(lines),
                lines.explored,
                lines.total
            )
        };
        layout.text(
            format!("{} · {progress}  [Show file diff]", file.path.display(),),
            palette.focus,
            Some(Control::CoverageFile(file.file)),
        );
        if let Some(index) = self.next_coverage_gap(remaining, file.file) {
            layout.text(
                "  [Next unexplored region]",
                palette.focus,
                Some(Control::CoverageGap(index)),
            );
        }
    }

    fn render_coverage_diff(
        &self,
        layout: &mut ConversationLayout,
        diff: &DiffComponent,
        palette: Palette,
    ) {
        if let Some(index) = self.coverage_file
            && let Some(file) = self
                .exploration
                .as_ref()
                .and_then(|pass| pass.comparison.files.get(index))
        {
            layout.gap();
            let start = layout.height;
            layout.text(
                format!("File diff · {}  [Close diff]", file.review_path().display()),
                palette.focus,
                Some(Control::CoverageReturn),
            );
            let view = Self::coverage_view();
            if let Some(viewer) = diff.evidence_view(view) {
                if let Some(limitation) = viewer.evidence_limitation() {
                    layout.text(limitation, palette.warning, None);
                }
                layout.push(
                    Content::Window(view),
                    layout.area.height.saturating_div(2).max(3),
                );
            } else {
                layout.text(
                    "Coverage diff is loading or unavailable",
                    palette.warning,
                    None,
                );
            }
            layout.coverage_diff = Some(start..layout.height.min(start.saturating_add(8)));
        }
    }

    pub fn render(
        &self,
        area: Rect,
        buffer: &mut Buffer,
        palette: Palette,
        focused: bool,
        diff: &DiffComponent,
    ) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if focused { palette.focus } else { palette.dim }));
        let content = block.inner(area);
        block.render(area, buffer);
        if self.conclusion_preview.is_some() {
            self.render_conclusion_preview(content, buffer, palette, focused, diff);
            return;
        }
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
        layout.render_frame(buffer, palette);
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
            Content::Window(view) => {
                if let Some(viewer) = diff.evidence_view(*view) {
                    viewer.render_embedded(
                        Window::new(*view, area, skipped, item.height).viewport,
                        buffer,
                        palette,
                        !focused && *view == self.view_id(),
                    );
                }
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
            Self::question_sections(question, layout, palette);
            self.evidence_block(index, layout, diff, palette);
        }
        if self.map {
            self.render_map(layout, palette);
        }
    }

    pub(super) fn execution_time(&self, selected_question: Option<&Question>) -> String {
        let Some(exploration) = &self.exploration else {
            return "Jev: — · Agent: —".into();
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
        let jev = self
            .coverage
            .as_ref()
            .filter(|coverage| coverage.classification_started)
            .map(|coverage| coverage.jev_elapsed_ms);
        format!(
            "Jev total {} · Agent total {}",
            jev.map_or_else(|| "—".into(), format_elapsed),
            agent.map_or_else(|| "—".into(), format_elapsed)
        )
    }

    fn render_jev_debug(&self, layout: &mut ConversationLayout, palette: Palette) {
        let (Some(coverage), Some(counts)) = (&self.coverage, self.coverage_cache.get()) else {
            return;
        };
        let exclusions = &counts.exclusions;
        layout.gap();
        if !self.jev_enabled && !self.completion_done {
            layout.text(
                "Jev disabled — all changes require coverage",
                palette.dim,
                Some(Control::JevDebug),
            );
        }
        let start = layout.height;
        layout.text(
            format!(
                "{} Jev exclusions · {} unexplored regions  [{}]",
                if self.jev_debug { "▾" } else { "▸" },
                exclusions.len(),
                if self.jev_debug { "Hide" } else { "Inspect" }
            ),
            palette.focus,
            Some(Control::JevDebug),
        );
        if !self.jev_debug {
            return;
        }
        self.render_jev_exclusions(layout, exclusions, palette);
        for result in coverage.classifications.values() {
            Self::render_jev_result(layout, result, palette);
        }
        layout.jev = Some(start..layout.height.min(start.saturating_add(4)));
    }

    fn render_jev_exclusions(
        &self,
        layout: &mut ConversationLayout,
        exclusions: &[review_explore::Gap],
        palette: Palette,
    ) {
        for (index, gap) in exclusions.iter().enumerate() {
            let lines = gap.location.lines.as_ref().map_or_else(
                || format!("{:?}", gap.kind),
                |range| format!("{}-{}", range.first_line, range.last_line),
            );
            layout.text(
                format!(
                    "{} {:?} {lines}  [Inspect diff]",
                    gap.location.path.display(),
                    gap.location.side
                ),
                palette.focus,
                Some(Control::ExcludedGap(index)),
            );
            if !self.completion_done {
                layout.text(
                    "[Require review]",
                    palette.focus,
                    Some(Control::RequireReview(index)),
                );
            }
        }
    }

    fn render_jev_result(
        layout: &mut ConversationLayout,
        result: &review_explore::SignificanceResult,
        palette: Palette,
    ) {
        layout.text(
            format!(
                "{} · {:?} · model {} · rubric {}",
                result.id,
                result.outcome,
                result.model.as_deref().unwrap_or("no provider response"),
                result.rubric,
            ),
            palette.text,
            None,
        );
        if !result.criterion.is_empty() {
            layout.text(
                format!("Criterion: {}", result.criterion),
                palette.dim,
                None,
            );
        }
        for reference in &result.input_references {
            layout.text(format!("Input: {reference}"), palette.dim, None);
        }
        for omission in &result.omissions {
            layout.text(format!("Omitted: {omission}"), palette.dim, None);
        }
        if !result.probabilities.is_empty() || result.confidence.is_some() {
            layout.text(
                format!(
                    "Probabilities: {:?} · confidence: {:?}",
                    result.probabilities, result.confidence
                ),
                palette.dim,
                None,
            );
        }
        if let Some(error) = &result.error {
            layout.text(format!("Diagnostic: {error}"), palette.warning, None);
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
