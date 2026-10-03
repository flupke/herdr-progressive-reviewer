use super::{ExploreComponent, flow::ConversationLayout};
use review_explore::{Question, ReviewerAnswer};
use ui_theme::Palette;

impl ExploreComponent {
    pub(super) fn initial_reply(&self) -> Option<&str> {
        let turn = self.exploration.as_ref()?.conversation.first()?;
        (turn.answer.is_none()
            && turn.update.conclusion.is_some()
            && self.general_context.as_ref() == Some(&turn.update.request))
        .then_some(turn.update.reply.as_ref()?.text.as_str())
    }

    pub(super) fn question_sections(
        question: &Question,
        layout: &mut ConversationLayout,
        palette: Palette,
    ) {
        layout.section("Context", &question.context(), palette);
        if let Some(assessment) = &question.assessments {
            for section in assessment.sections() {
                layout.section(section.title, &section.body, palette);
            }
        }
    }

    pub(super) fn agent_reply(
        &self,
        answer: &ReviewerAnswer,
        layout: &mut ConversationLayout,
        palette: Palette,
    ) {
        let exploration = self.exploration.as_ref().expect("reply exploration");
        for turn in exploration
            .conversation
            .iter()
            .filter(|turn| turn.answer.as_ref() == Some(&answer.id) && turn.update.next.is_none())
        {
            self.agent_turn(turn, layout, palette);
        }
    }

    pub(super) fn preceding_reply(
        &self,
        index: usize,
        layout: &mut ConversationLayout,
        palette: Palette,
    ) {
        let exploration = self.exploration.as_ref().expect("reply exploration");
        for turn in exploration
            .conversation
            .iter()
            .filter(|turn| turn.update.next.as_ref() == exploration.questions.get(index))
        {
            self.agent_turn(turn, layout, palette);
        }
    }

    /// The marks the turn asking question `index` holds until the reviewer
    /// answers it; once answered, they show below the answer that applied
    /// them. Marks no answer applied, from rounds saved before marks waited
    /// for an answer, show here too.
    pub(super) fn question_marks(
        &self,
        index: usize,
        layout: &mut ConversationLayout,
        palette: Palette,
    ) {
        let exploration = self.exploration.as_ref().expect("question exploration");
        for (position, turn) in exploration.conversation.iter().enumerate() {
            if turn.update.next.as_ref() != exploration.questions.get(index) {
                continue;
            }
            let request = &turn.update.request;
            match self.marks.get(request) {
                Some(marks) if marks.answer.is_none() => {
                    self.turn_marks(position, layout, palette);
                }
                Some(_) => {}
                None => {
                    let answered = exploration
                        .answers
                        .iter()
                        .any(|answer| &answer.in_reply_to == request);
                    // A round from history takes no answer, so nothing is pending.
                    if !answered && !self.durable.historical {
                        self.pending_marks(position, layout, palette);
                    }
                }
            }
        }
    }

    fn agent_turn(
        &self,
        turn: &review_explore::ConversationTurn,
        layout: &mut ConversationLayout,
        palette: Palette,
    ) {
        let exploration = self.exploration.as_ref().expect("reply exploration");
        if let Some(reply) = &turn.update.reply {
            layout.labelled_prose("Agent", &reply.text, palette);
        }
        for change in &turn.update.agenda {
            layout.text(
                format!(
                    "{:?} · {}: {}",
                    change.action, exploration.topics[&change.topic].title, change.reason
                ),
                palette.dim,
                None,
            );
        }
        layout.gap();
    }

    pub(super) fn agenda_map(&self, layout: &mut ConversationLayout, palette: Palette) {
        let exploration = self.exploration.as_ref().expect("agenda exploration");
        let mut topics: Vec<_> = exploration.topics.values().collect();
        topics.sort_by_key(|topic| (topic.rank, &topic.id));
        layout.text(
            "Agenda · associations are navigation; they do not mark lines reviewed.",
            palette.dim,
            None,
        );
        for topic in topics {
            layout.gap();
            layout.text(
                format!("{}: {}", exploration.agenda_label(&topic.id), topic.title),
                palette.text,
                None,
            );
            if !topic.prompt.is_empty() {
                layout.text(&topic.prompt, palette.dim, None);
            }
            if !topic.prerequisites.is_empty() {
                let titles: Vec<_> = topic
                    .prerequisites
                    .iter()
                    .map(|id| exploration.topics[id].title.as_str())
                    .collect();
                layout.text(
                    format!("Depends on: {}", titles.join(", ")),
                    palette.dim,
                    None,
                );
            }
            if let Some(change) = exploration.agenda_change(&topic.id) {
                layout.text(&change.reason, palette.dim, None);
                if let Some(replacement) = &change.replacement {
                    layout.text(
                        format!("Replaced by: {}", exploration.topics[replacement].title),
                        palette.dim,
                        None,
                    );
                }
            }
        }
    }
}
