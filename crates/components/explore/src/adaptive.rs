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
        let context = [&question.rationale, &question.visual]
            .into_iter()
            .flatten()
            .map(String::as_str)
            .filter(|text| !text.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        layout.section("Context", &context, palette);
        if let Some(assessment) = &question.assessments {
            for (title, mut body, lens) in [
                (
                    "Door",
                    format!(
                        "{} — {}",
                        assessment.door.label(),
                        assessment.reversibility.summary
                    ),
                    &assessment.reversibility,
                ),
                (
                    "Blast radius",
                    assessment.blast_radius.summary.clone(),
                    &assessment.blast_radius,
                ),
            ] {
                if !lens.details.trim().is_empty() {
                    body.push_str("\n\n");
                    body.push_str(&lens.details);
                }
                for unknown in &lens.unknowns {
                    body.push_str("\n\nUnknown: ");
                    body.push_str(unknown);
                }
                layout.section(title, &body, palette);
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

    fn agent_turn(
        &self,
        turn: &review_explore::ConversationTurn,
        layout: &mut ConversationLayout,
        palette: Palette,
    ) {
        let exploration = self.exploration.as_ref().expect("reply exploration");
        if let Some(reply) = &turn.update.reply {
            layout.gap();
            layout.text(format!("Agent: {}", reply.text), palette.text, None);
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
            "Agenda · associations are navigation; Files still requires human inspection.",
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
