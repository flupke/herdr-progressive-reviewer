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
            conclusion.validate_quiz(comparison)?;
        }
        self.validate_design(update)?;
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
            let id = &topic.id;
            eyre::ensure!(!id.is_empty(), "A topic needs an ID");
            eyre::ensure!(ids.insert(id), "Topic {id} is sent twice: send it once");
            eyre::ensure!(!topic.title.trim().is_empty(), "Topic {id} needs a title");
            eyre::ensure!(
                topic
                    .entries
                    .iter()
                    .all(|location| comparison.validate_location(location)),
                "Topic {id} references an invalid code location"
            );
            if let Some(previous) = self.topics.get(id) {
                eyre::ensure!(
                    previous.status == TopicStatus::Open || previous.same_but_status(topic),
                    "Topic {id} is settled and cannot be rewritten: leave it out of topics"
                );
            }
        }
        self.validate_glossary(&update.glossary)?;
        self.validate_agenda(update, comparison)?;
        self.validate_marks(update, comparison)?;
        self.validate_challenger_proposals(update)?;
        if let Some(question) = &update.next {
            self.validate_question(question, update, comparison)?;
        }
        Ok(())
    }

    /// Marks name lines of changed paths; those an answer settled or
    /// reopened follow a human answer.
    fn validate_marks(
        &self,
        update: &InterviewUpdate,
        comparison: &Comparison,
    ) -> eyre::Result<()> {
        eyre::ensure!(
            (update.reviewed.is_empty() && update.reopened.is_empty())
                || self
                    .outstanding
                    .as_ref()
                    .is_some_and(|turn| turn.answer.is_some()),
            "reviewed and reopened follow a human answer; this turn has none to mark lines from"
        );
        eyre::ensure!(
            update
                .reviewed
                .iter()
                .chain(&update.reopened)
                .chain(crate::NotRelevantMark::locations(&update.not_relevant))
                .all(|location| comparison.validate_mark(location)),
            "reviewed, reopened and not_relevant need changed paths, on the side that has them, \
             with valid lines"
        );
        update
            .not_relevant
            .iter()
            .try_for_each(|mark| mark.validate(comparison))
    }

    /// The first question comes after the design of the change, and only the first turn
    /// explains it. The conclusion tool has no design: a first turn that concludes puts it in
    /// the summary.
    fn validate_design(&self, update: &InterviewUpdate) -> eyre::Result<()> {
        let kickoff = self
            .outstanding
            .as_ref()
            .is_some_and(crate::TurnRequest::is_kickoff);
        let Some(design) = &update.design else {
            eyre::ensure!(
                !kickoff || update.next.is_none(),
                "The first question comes after the design of the change: fill design"
            );
            return Ok(());
        };
        eyre::ensure!(
            kickoff,
            "Only the first turn explains the design; leave design null and correct it in reply"
        );
        design.validate()
    }

    /// Only a round with a challenger reports its proposals.
    fn validate_challenger_proposals(&self, update: &InterviewUpdate) -> eyre::Result<()> {
        eyre::ensure!(
            self.challenger || update.challenger_proposals.is_empty(),
            "challenger_proposals belong to a round with a challenger; this round has none"
        );
        update
            .challenger_proposals
            .iter()
            .try_for_each(crate::ChallengerProposal::validate)
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
                    answer
                        .option
                        .as_ref()
                        .is_none_or(|option| option.outcome == TopicStatus::Open)
                        || !answer.text.is_empty(),
                    "An explicit decision needs an interpretation"
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
            _ => {
                eyre::bail!("An interpretation is required only for the submitted human answer");
            }
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
                        && !evidence.notes.trim().is_empty()),
            "Question evidence needs valid sources/ranges and nonempty notes"
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
        if let Some((choice, mark)) = question
            .alternatives
            .iter()
            .find_map(|choice| choice.recommended_mark().map(|mark| (choice, mark)))
        {
            eyre::bail!(
                "Choice \"{}\" says in its text that it is recommended: remove \"{mark}\" from \
                 its text. A choice's text never says that it is recommended, preferred or the \
                 default; the recommendation goes only in its recommendation field",
                choice.id
            );
        }
        self.validate_ask_author(question)?;
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
            "Do not ask again on a topic just decided"
        );
        eyre::ensure!(
            status == Some(TopicStatus::Open)
                || agenda.is_some_and(|change| change.action == AgendaAction::Reconsider),
            "Agent cannot reopen a settled topic without reconsidering it"
        );
        Ok(())
    }

    /// A question about someone else's change can be left to its author.
    fn validate_ask_author(&self, question: &Question) -> eyre::Result<()> {
        eyre::ensure!(
            self.author == crate::ChangeAuthor::Reviewer
                || question.alternatives.iter().any(|choice| {
                    choice.id == crate::ASK_AUTHOR && choice.outcome == TopicStatus::Open
                }),
            "In a review of someone else's change, every question offers the choice \"{}\", \
             with outcome open: \"Ask the author: \" and the question to ask them",
            crate::ASK_AUTHOR
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
        self.topic_status(topic).or_else(|| {
            update
                .topics
                .iter()
                .any(|item| item.id == topic)
                .then_some(TopicStatus::Open)
        })
    }
}

/// The shortest glossary term, in characters: shorter ones would mark common words.
const TERM_SHORTEST: usize = 2;
/// The longest glossary term, in characters: a term is a word or a short phrase of the change.
const TERM_LENGTH: usize = 60;
/// The longest definition, in characters: one plain sentence.
const DEFINITION_LENGTH: usize = 300;
/// The most terms a round's glossary holds.
const TERMS: usize = 100;

impl Exploration {
    /// Each term is a short phrase on one line, with a definition of one sentence, and no two
    /// terms of the round differ only by case or spaces: the page takes them for one term.
    fn validate_glossary(
        &self,
        glossary: &std::collections::BTreeMap<String, String>,
    ) -> eyre::Result<()> {
        for (term, definition) in glossary {
            validate_term(term, definition)?;
            let same = self
                .glossary
                .keys()
                .chain(glossary.keys())
                .find(|other| same_term(other, term) && *other != term);
            if let Some(same) = same {
                eyre::bail!(
                    "Glossary term {term:?} differs from {same:?} only by case or spaces: send one \
                     of them, as the page marks a term whatever its case and spaces"
                );
            }
        }
        let terms = self
            .glossary
            .keys()
            .chain(glossary.keys())
            .collect::<HashSet<_>>()
            .len();
        eyre::ensure!(
            terms <= TERMS,
            "The glossary would hold {terms} terms, above {TERMS}: keep the terms a reviewer new \
             to the code needs"
        );
        Ok(())
    }
}

/// Whether the page would take `a` and `b` for one term: the same words whatever their case and
/// the spaces between them.
fn same_term(a: &str, b: &str) -> bool {
    let words = |term: &str| {
        term.split_whitespace()
            .map(str::to_lowercase)
            .collect::<Vec<_>>()
    };
    words(a) == words(b)
}

/// `term` is a short phrase on one line, and `definition` one sentence.
fn validate_term(term: &str, definition: &str) -> eyre::Result<()> {
    eyre::ensure!(
        term.trim() == term
            && (TERM_SHORTEST..=TERM_LENGTH).contains(&term.chars().count())
            && !term.chars().any(char::is_control),
        "Glossary term {term:?} must be a word or a short phrase on one line, without spaces \
         around it, of {TERM_SHORTEST} to {TERM_LENGTH} characters"
    );
    eyre::ensure!(
        !definition.trim().is_empty() && definition.chars().count() <= DEFINITION_LENGTH,
        "The definition of glossary term {term:?} must be one plain sentence of at most \
         {DEFINITION_LENGTH} characters"
    );
    Ok(())
}
