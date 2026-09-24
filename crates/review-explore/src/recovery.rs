use crate::Exploration;
use std::collections::HashSet;

impl Exploration {
    /// Validate stored structure without reopening or checking the freshness of sources.
    fn validate_restored(&self) -> eyre::Result<()> {
        eyre::ensure!(!self.instance.is_empty(), "missing pass identity");
        let mut requests = HashSet::new();
        let mut questions = Vec::new();
        for turn in &self.conversation {
            eyre::ensure!(
                turn.update.instance == self.instance
                    && turn.update.checkpoint == self.comparison.checkpoint
                    && requests.insert(&turn.update.request),
                "invalid saved turn identity"
            );
            eyre::ensure!(
                turn.answer
                    .as_ref()
                    .is_none_or(|id| self.answers.iter().any(|answer| &answer.id == id)),
                "saved turn lost its original answer"
            );
            if let Some(question) = &turn.update.next {
                questions.push(question.clone());
            }
        }
        eyre::ensure!(
            questions == self.questions,
            "saved question history is inconsistent"
        );
        self.validate_saved_agenda()?;
        let mut answers = HashSet::new();
        for answer in &self.answers {
            eyre::ensure!(
                answer.checkpoint == self.comparison.checkpoint && answers.insert(&answer.id),
                "invalid saved answer identity"
            );
            eyre::ensure!(
                answer
                    .question
                    .as_ref()
                    .is_none_or(|question| self.questions.contains(question)),
                "saved answer lost its question version"
            );
            self.validate_saved_answer(answer)?;
        }
        eyre::ensure!(
            self.interpretations
                .iter()
                .all(|item| answers.contains(&item.answer)),
            "saved interpretation lost its answer"
        );
        for request in self.outstanding.iter().chain(self.retry.iter()) {
            eyre::ensure!(
                request.instance == self.instance
                    && request.checkpoint == self.comparison.checkpoint
                    && request
                        .answer
                        .as_ref()
                        .is_none_or(|answer| self.answers.contains(answer)),
                "invalid saved pending turn"
            );
        }
        eyre::ensure!(
            self.comparison
                .manifest
                .iter()
                .all(|entry| entry.file < self.comparison.files.len()
                    && entry.hunk.is_none_or(|hunk| hunk > 0)),
            "invalid saved comparison mapping"
        );
        Ok(())
    }

    fn validate_saved_agenda(&self) -> eyre::Result<()> {
        let known_topic = |id: &String| self.topics.contains_key(id);
        let known_answer = |id: &String| self.answers.iter().any(|answer| &answer.id == id);
        eyre::ensure!(
            self.topics.iter().all(|(id, topic)| id == &topic.id),
            "invalid saved topic identity"
        );
        for topic in self.topics.values().chain(
            self.conversation
                .iter()
                .flat_map(|turn| &turn.update.topics),
        ) {
            eyre::ensure!(
                known_topic(&topic.id) && topic.prerequisites.iter().all(known_topic),
                "saved agenda lost a topic or prerequisite"
            );
        }
        eyre::ensure!(
            self.questions
                .iter()
                .all(|question| known_topic(&question.topic)),
            "saved question lost its topic"
        );
        for change in self
            .conversation
            .iter()
            .flat_map(|turn| &turn.update.agenda)
        {
            eyre::ensure!(
                known_topic(&change.topic) && change.replacement.as_ref().is_none_or(known_topic),
                "saved agenda lost its topic or replacement"
            );
            eyre::ensure!(
                change.answer.as_ref().is_none_or(known_answer)
                    && change.decision.as_ref().is_none_or(known_answer),
                "saved agenda lost its attribution"
            );
        }
        Ok(())
    }

    fn validate_saved_answer(&self, answer: &crate::ReviewerAnswer) -> eyre::Result<()> {
        let context = self
            .conversation
            .iter()
            .find(|turn| turn.update.request == answer.in_reply_to);
        eyre::ensure!(
            context.is_some_and(|turn| match &answer.question {
                Some(question) => turn.update.next.as_ref() == Some(question),
                None => turn.update.conclusion.is_some(),
            }),
            "saved answer lost its original context"
        );
        if let Some(option) = &answer.option {
            eyre::ensure!(
                answer
                    .question
                    .as_ref()
                    .is_some_and(|q| q.choices().any(|choice| choice == option)),
                "saved answer lost its exact option"
            );
        }
        eyre::ensure!(
            answer
                .corrects
                .as_ref()
                .is_none_or(|id| id != &answer.id && self.answers.iter().any(|old| &old.id == id)),
            "saved correction lost its original answer"
        );
        Ok(())
    }

    pub fn pending_request(&self) -> Option<&crate::TurnRequest> {
        self.outstanding.as_ref()
    }

    pub fn retry_request(&self) -> Option<&crate::TurnRequest> {
        self.retry.as_ref()
    }

    /// Runtime dispatch is interrupted by reopening; the posted answer stays posted.
    pub fn pause_delivery(&mut self) {
        self.outstanding = None;
    }
}

impl crate::ExplorePass {
    pub fn validate_restored(&self) -> eyre::Result<()> {
        self.exploration.validate_restored()?;
        self.coverage.validate_restored(
            &self.exploration.comparison,
            self.exploration
                .answers
                .iter()
                .map(|answer| answer.id.clone()),
        )?;
        if let Some(completion) = &self.completion {
            eyre::ensure!(
                self.exploration.conversation.iter().any(|turn| {
                    turn.update.request == completion.request && turn.update.conclusion.is_some()
                }) && self.exploration.comparison.checkpoint.checkpoint == completion.baseline,
                "saved completion lost its conclusion or baseline"
            );
            let expected: std::collections::BTreeSet<_> = self
                .exploration
                .comparison
                .files
                .iter()
                .map(|file| file.review_path().as_bytes().to_vec())
                .collect();
            let actual: std::collections::BTreeSet<_> = completion
                .marks
                .iter()
                .map(|mark| mark.path.clone())
                .collect();
            eyre::ensure!(
                expected == actual
                    && actual.len() == completion.marks.len()
                    && (!completion.completed || completion.marks.iter().all(|mark| mark.applied)),
                "saved completion has incorrect file targets"
            );
            if completion.summary.complete {
                eyre::ensure!(
                    self.coverage_receipts
                        .get(&completion.request)
                        .is_some_and(|receipt| receipt.feedback().summary == completion.summary),
                    "saved completion coverage receipt is inconsistent"
                );
            }
        }
        for (id, record) in &self.turns {
            eyre::ensure!(
                *id == record.request.request
                    && record.request.instance == self.exploration.instance
                    && record.request.checkpoint == self.exploration.comparison.checkpoint
                    && !record.attempt.is_empty(),
                "invalid saved delivery identity"
            );
            eyre::ensure!(
                record
                    .request
                    .answer
                    .as_ref()
                    .is_none_or(|answer| self.exploration.answers.contains(answer)),
                "saved delivery lost its exact answer"
            );
        }
        for (id, record) in &self.implementations {
            eyre::ensure!(
                *id == record.request.delivery
                    && record.request.instance == self.exploration.instance
                    && !record.attempt.is_empty(),
                "invalid saved implementation identity"
            );
            eyre::ensure!(
                self.exploration
                    .conversation
                    .iter()
                    .any(|turn| turn.update.request == record.request.conclusion
                        && turn.update.conclusion.is_some()),
                "saved implementation lost its conclusion"
            );
        }
        Ok(())
    }
}
