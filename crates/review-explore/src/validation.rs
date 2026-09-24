use crate::{AgendaAction, Comparison, Exploration, InterviewUpdate, Question, TopicStatus};
use std::collections::HashSet;

impl Exploration {
    pub(crate) fn validate(
        &self,
        update: &InterviewUpdate,
        comparison: &Comparison,
    ) -> eyre::Result<()> {
        eyre::ensure!(
            update.checkpoint == self.comparison.checkpoint,
            "Response checkpoint does not match"
        );
        eyre::ensure!(
            update.next.is_some() != update.conclusion.is_some(),
            "Return one question or an explicit closing state"
        );
        self.validate_interpretation(update)?;
        if let Some(conclusion) = &update.conclusion {
            eyre::ensure!(
                !conclusion.summary.trim().is_empty(),
                "Conclusion summary is required"
            );
        }
        if self
            .outstanding
            .as_ref()
            .is_some_and(|turn| turn.answer.is_some())
        {
            eyre::ensure!(
                update.reply.is_some() || update.conclusion.is_some(),
                "Respond directly to the reviewer contribution"
            );
        }
        if let Some(reply) = &update.reply {
            eyre::ensure!(
                !reply.text.trim().is_empty()
                    && reply
                        .evidence
                        .iter()
                        .all(|evidence| comparison.validate_evidence(evidence)),
                "Direct reply needs text and valid evidence"
            );
        }
        let mut ids = HashSet::new();
        for topic in &update.topics {
            eyre::ensure!(
                !topic.id.is_empty() && !topic.title.trim().is_empty() && ids.insert(&topic.id),
                "Invalid or duplicate topic ID"
            );
            eyre::ensure!(
                topic
                    .entries
                    .iter()
                    .all(|location| comparison.validate_location(location)),
                "Topic references an invalid code location"
            );
            if let Some(previous) = self.topics.get(&topic.id) {
                eyre::ensure!(
                    previous.status == topic.status,
                    "Only an attributed interpretation may change topic status"
                );
                eyre::ensure!(
                    matches!(previous.status, TopicStatus::Open | TopicStatus::Deferred)
                        || previous == topic,
                    "Settled topics cannot be rewritten"
                );
            } else {
                eyre::ensure!(topic.status == TopicStatus::Open, "New topics must be open");
            }
        }
        self.validate_agenda(update, comparison)?;
        if let Some(question) = &update.next {
            self.validate_question(question, update, comparison)?;
        }
        Ok(())
    }

    fn validate_interpretation(&self, update: &InterviewUpdate) -> eyre::Result<()> {
        let answer = self
            .outstanding
            .as_ref()
            .and_then(|turn| turn.answer.as_ref());
        match (answer, &update.interpretation) {
            (None, None) => Ok(()),
            (Some(answer), None) => {
                eyre::ensure!(
                    !answer.deferred
                        && (answer
                            .option
                            .as_ref()
                            .is_none_or(|option| option.outcome == TopicStatus::Open)
                            || !answer.text.is_empty()),
                    "An explicit decision or deferral needs an interpretation"
                );
                Ok(())
            }
            (Some(answer), Some(interpretation)) => {
                eyre::ensure!(
                    answer.question.is_some(),
                    "Conversation context is not a policy decision"
                );
                eyre::ensure!(
                    interpretation.answer == answer.id && !interpretation.recap.trim().is_empty(),
                    "Interpretation must reference the exact submitted answer"
                );
                if answer.deferred {
                    eyre::ensure!(
                        interpretation.status == TopicStatus::Deferred,
                        "A deferred answer must remain deferred"
                    );
                }
                if answer.text.is_empty()
                    && let Some(option) = &answer.option
                {
                    eyre::ensure!(
                        interpretation.status == option.outcome,
                        "An explicit choice must keep its stated outcome"
                    );
                }
                if !interpretation.follow_ups.is_empty() {
                    eyre::ensure!(
                        interpretation.status == TopicStatus::NeedsFollowUp,
                        "Required changes must remain follow-ups"
                    );
                }
                Ok(())
            }
            _ => eyre::bail!("An interpretation is required only for the submitted human answer"),
        }
    }

    fn validate_question(
        &self,
        question: &Question,
        update: &InterviewUpdate,
        comparison: &Comparison,
    ) -> eyre::Result<()> {
        eyre::ensure!(
            !question.id.is_empty() && question.version > 0 && !question.text.trim().is_empty(),
            "Question identity and text are required"
        );
        eyre::ensure!(
            self.topics.contains_key(&question.topic)
                || update.topics.iter().any(|topic| topic.id == question.topic),
            "Question has an unknown topic"
        );
        eyre::ensure!(
            !self.questions.iter().any(|old| old.id == question.id
                && (old.version >= question.version || old.topic != question.topic)),
            "A displayed question version is immutable"
        );
        eyre::ensure!(
            !question.evidence.is_empty()
                && question
                    .evidence
                    .iter()
                    .all(|evidence| comparison.validate_evidence(evidence)
                        && !evidence.notes.trim().is_empty())
                && question
                    .supporting
                    .iter()
                    .all(|evidence| comparison.validate_evidence(evidence)),
            "Question evidence and supporting references need valid sources/ranges and nonempty notes"
        );
        let mut locations = HashSet::new();
        eyre::ensure!(
            question.evidence.iter().all(|evidence| locations.insert((
                &evidence.location.path,
                evidence.location.side,
                evidence
                    .location
                    .lines
                    .as_ref()
                    .map(|lines| (lines.first_line, lines.last_line))
            ))),
            "Combine duplicate snippets; each displayed excerpt must add distinct decision-relevant information"
        );
        let mut ids = HashSet::new();
        let mut texts = HashSet::new();
        eyre::ensure!(
            (2..=5).contains(&question.alternatives.len())
                && question.choices().all(|option| !option.id.is_empty()
                    && !option.text.trim().is_empty()
                    && ids.insert(&option.id)
                    && texts.insert(option.text.trim().to_lowercase())),
            "Every question needs two to five distinct choices with unique IDs and text; None of the above is provided by the reviewer"
        );
        if let Some(assessments) = &question.assessments {
            assessments.validate(comparison)?;
        }
        let agenda = update
            .agenda
            .iter()
            .find(|change| change.topic == question.topic)
            .or_else(|| self.agenda_change(&question.topic));
        eyre::ensure!(
            !agenda.is_some_and(|change| matches!(
                change.action,
                AgendaAction::Retire | AgendaAction::Supersede
            )),
            "Next question belongs to an inactive agenda topic"
        );
        let status = self.question_topic_status(&question.topic, update);
        eyre::ensure!(
            !update
                .interpretation
                .as_ref()
                .is_some_and(|interpretation| interpretation.status != TopicStatus::Open
                    && self
                        .outstanding
                        .as_ref()
                        .and_then(|turn| turn.answer.as_ref())
                        .is_some_and(|answer| answer
                            .question
                            .as_ref()
                            .is_some_and(|answered| answered.topic == question.topic))),
            "Do not ask again on a topic just decided or deferred"
        );
        eyre::ensure!(
            matches!(status, Some(TopicStatus::Open | TopicStatus::Deferred))
                || agenda.is_some_and(|change| change.action == AgendaAction::Reconsider),
            "Agent cannot reopen a settled topic without a human correction"
        );
        Ok(())
    }

    fn question_topic_status(&self, topic: &str, update: &InterviewUpdate) -> Option<TopicStatus> {
        if self
            .outstanding
            .as_ref()
            .and_then(|request| request.answer.as_ref())
            .is_some_and(|answer| {
                answer
                    .question
                    .as_ref()
                    .is_some_and(|question| question.topic == topic)
            })
            && let Some(interpretation) = &update.interpretation
        {
            return Some(interpretation.status);
        }
        update
            .topics
            .iter()
            .find(|item| item.id == topic)
            .or_else(|| self.topics.get(topic))
            .map(|topic| topic.status)
    }
}
