use crate::{AgendaAction, AgendaChange, Comparison, Exploration, InterviewUpdate, Topic};
use std::collections::{BTreeMap, HashSet};

impl Exploration {
    pub(crate) fn validate_agenda(
        &self,
        update: &InterviewUpdate,
        comparison: &Comparison,
    ) -> eyre::Result<()> {
        let mut topics: BTreeMap<_, _> = self.topics.iter().collect();
        for topic in &update.topics {
            if self.agenda_change(&topic.id).is_some_and(|change| {
                matches!(
                    change.action,
                    AgendaAction::Retire | AgendaAction::Supersede
                )
            }) {
                eyre::ensure!(
                    self.topics.get(&topic.id) == Some(topic),
                    "Inactive topic wording must be preserved"
                );
            }
            topics.insert(&topic.id, topic);
        }
        for topic in topics.values() {
            topic.validate_prerequisites(&topics)?;
        }
        let mut changed = HashSet::new();
        for change in &update.agenda {
            eyre::ensure!(
                topics.contains_key(&change.topic) && changed.insert(&change.topic),
                "Unknown or duplicate agenda target"
            );
            change.validate_reason(self, comparison)?;
            match change.action {
                AgendaAction::Supersede => {
                    let replacement = change
                        .replacement
                        .as_ref()
                        .ok_or_else(|| eyre::eyre!("Supersede needs a replacement"))?;
                    eyre::ensure!(
                        replacement != &change.topic && topics.contains_key(replacement),
                        "Invalid replacement topic"
                    );
                    let lifecycle = update
                        .agenda
                        .iter()
                        .find(|item| &item.topic == replacement)
                        .or_else(|| self.agenda_change(replacement));
                    eyre::ensure!(
                        !lifecycle.is_some_and(|item| matches!(
                            item.action,
                            AgendaAction::Retire | AgendaAction::Supersede
                        )),
                        "Replacement topic must remain active"
                    );
                }
                _ => eyre::ensure!(
                    change.replacement.is_none(),
                    "Only supersede has a replacement"
                ),
            }
            change.validate_decision(self)?;
        }
        Ok(())
    }
}

impl Topic {
    fn validate_prerequisites(&self, topics: &BTreeMap<&String, &Topic>) -> eyre::Result<()> {
        let mut pending = self.prerequisites.clone();
        let mut visited = HashSet::new();
        while let Some(id) = pending.pop() {
            eyre::ensure!(id != self.id, "Agenda prerequisites cannot form a cycle");
            let prerequisite = topics
                .get(&id)
                .ok_or_else(|| eyre::eyre!("Unknown agenda prerequisite"))?;
            if visited.insert(id) {
                pending.extend(prerequisite.prerequisites.clone());
            }
        }
        Ok(())
    }
}
impl AgendaChange {
    fn validate_reason(
        &self,
        exploration: &Exploration,
        comparison: &Comparison,
    ) -> eyre::Result<()> {
        eyre::ensure!(
            !self.reason.trim().is_empty() && (self.answer.is_some() || !self.evidence.is_empty()),
            "Agenda changes need an attributed reason"
        );
        eyre::ensure!(
            self.answer
                .as_ref()
                .is_none_or(|id| exploration.answers.iter().any(|answer| &answer.id == id))
                && self
                    .evidence
                    .iter()
                    .all(|evidence| comparison.validate_evidence(evidence)),
            "Unknown agenda reason reference"
        );
        Ok(())
    }
    fn validate_decision(&self, exploration: &Exploration) -> eyre::Result<()> {
        if let Some(decision) = &self.decision {
            eyre::ensure!(
                self.action == AgendaAction::Reconsider
                    && exploration
                        .interpretations
                        .iter()
                        .any(|interpretation| &interpretation.answer == decision)
                    && exploration
                        .answers
                        .iter()
                        .any(|answer| &answer.id == decision
                            && answer
                                .question
                                .as_ref()
                                .is_some_and(|question| question.topic == self.topic)),
                "Reconsideration must reference a decision on this topic"
            );
        }
        if self.action == AgendaAction::Reconsider
            && exploration.interpretations.iter().any(|interpretation| {
                exploration.answers.iter().any(|answer| {
                    answer.id == interpretation.answer
                        && answer
                            .question
                            .as_ref()
                            .is_some_and(|question| question.topic == self.topic)
                })
            })
        {
            eyre::ensure!(
                self.decision.is_some(),
                "Preserve the decision being reconsidered"
            );
        }
        Ok(())
    }
}
