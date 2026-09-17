use crate::{EvidenceRef, Exploration, Question, TopicStatus};
use serde::{Deserialize, Serialize};

/// Agenda lifecycle is independent of the reviewer's recorded decision.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AgendaAction {
    Retire,
    Supersede,
    Reconsider,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AgendaChange {
    pub topic: String,
    pub action: AgendaAction,
    pub reason: String,
    pub answer: Option<String>,
    pub evidence: Vec<EvidenceRef>,
    pub replacement: Option<String>,
    /// The answer whose recorded decision needs reconsideration, if there is one.
    pub decision: Option<String>,
}

impl Exploration {
    pub fn can_choose(&self, question: &Question) -> bool {
        let active = match self
            .agenda_change(&question.topic)
            .map(|change| change.action)
        {
            Some(AgendaAction::Retire | AgendaAction::Supersede) => false,
            Some(AgendaAction::Reconsider) => true,
            None => self.topics.get(&question.topic).is_some_and(|topic| {
                matches!(topic.status, TopicStatus::Open | TopicStatus::Deferred)
            }),
        };
        active
            && !self
                .answers
                .iter()
                .any(|answer| answer.question.as_ref() == Some(question))
            && !self
                .questions
                .iter()
                .any(|newer| newer.id == question.id && newer.version > question.version)
    }
    pub fn agenda_change(&self, topic: &str) -> Option<&AgendaChange> {
        for turn in self.conversation.iter().rev() {
            if let Some(change) = turn
                .update
                .agenda
                .iter()
                .find(|change| change.topic == topic)
            {
                return Some(change);
            }
            if turn
                .update
                .interpretation
                .as_ref()
                .is_some_and(|interpretation| {
                    interpretation.status != TopicStatus::Open
                        && self.answers.iter().any(|answer| {
                            answer.id == interpretation.answer
                                && answer
                                    .question
                                    .as_ref()
                                    .is_some_and(|question| question.topic == topic)
                        })
                })
            {
                // A subsequent reviewer decision resolves a reconsideration, never erases it.
                let change = self
                    .conversation
                    .iter()
                    .rev()
                    .flat_map(|turn| &turn.update.agenda)
                    .find(|change| change.topic == topic);
                return change.filter(|change| change.action != AgendaAction::Reconsider);
            }
        }
        None
    }

    pub fn agenda_label(&self, topic: &str) -> &'static str {
        match self.agenda_change(topic).map(|change| change.action) {
            Some(AgendaAction::Retire) => "Retired",
            Some(AgendaAction::Supersede) => "Superseded",
            Some(AgendaAction::Reconsider) => "Reconsider",
            None => match self.topics[topic].status {
                TopicStatus::Open => "Outstanding",
                TopicStatus::Accepted => "Accepted",
                TopicStatus::NeedsFollowUp => "Follow-up required",
                TopicStatus::Deferred => "Deferred · outstanding",
            },
        }
    }
}
