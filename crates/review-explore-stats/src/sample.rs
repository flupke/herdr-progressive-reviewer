//! What one saved round measured, read from its record.
use crate::{summary::Share, words::Words};
use review_explore::{Exploration, ExploreRound, Question, ReviewerAnswer, TopicStatus};
use review_store::SavedRound;
use std::time::UNIX_EPOCH;

/// One accepted agent turn and how long it took.
pub(crate) struct AgentTurn {
    pub(crate) milliseconds: u64,
    /// The turn followed a reviewer's answer; only the first turn does not.
    pub(crate) after_answer: bool,
}

/// The measures of one saved round.
pub(crate) struct RoundSample {
    /// When the round's first prompt went out, or when it was saved when its
    /// turns carry no time.
    pub(crate) started_at_ms: u64,
    pub(crate) challenger: bool,
    pub(crate) answered: bool,
    pub(crate) concluded: bool,
    pub(crate) questions: usize,
    /// Answers to a question that the agent's next turn took up, and those it
    /// interpreted as asking for a change.
    pub(crate) change_requests: Share,
    /// Answers to a question with a recommended choice, and those that chose
    /// something else.
    pub(crate) declined_recommendations: Share,
    pub(crate) agent_turns: Vec<AgentTurn>,
    /// From the agent's accepted question to the reviewer's answer going out.
    pub(crate) reviewer_answers_ms: Vec<u64>,
    /// The words of each question's page that the agent wrote.
    pub(crate) question_words: Vec<usize>,
}

impl RoundSample {
    pub(crate) fn of(saved: &SavedRound) -> Self {
        let round = &saved.round;
        let exploration = &round.exploration;
        let saved_at_ms = saved
            .saved_at
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| {
                u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
            });
        Self {
            started_at_ms: round
                .turns
                .values()
                .filter_map(|turn| turn.started_at_ms)
                .min()
                .unwrap_or(saved_at_ms),
            challenger: exploration.challenger,
            answered: !exploration.answers.is_empty(),
            concluded: exploration.conclusion.is_some(),
            questions: exploration.questions.len(),
            change_requests: Self::change_requests(exploration),
            declined_recommendations: Self::declined_recommendations(exploration),
            agent_turns: Self::agent_turns(exploration),
            reviewer_answers_ms: exploration
                .answers
                .iter()
                .filter_map(|answer| Self::reviewer_answer(round, answer))
                .collect(),
            question_words: exploration
                .questions
                .iter()
                .map(|question| Self::question_words(exploration, question))
                .collect(),
        }
    }

    /// Over the answers to a question that the agent's next turn took up: an
    /// answer it left uninterpreted, as it may for free text, asked for nothing.
    fn change_requests(exploration: &Exploration) -> Share {
        let mut share = Share::default();
        for answer in exploration.answers.iter().filter(|a| a.question.is_some()) {
            let taken_up = exploration
                .conversation
                .iter()
                .any(|turn| turn.answer.as_ref() == Some(&answer.id));
            if taken_up {
                share.add(exploration.interpretations.iter().any(|interpretation| {
                    interpretation.answer == answer.id
                        && interpretation.status == TopicStatus::NeedsFollowUp
                }));
            }
        }
        share
    }

    fn declined_recommendations(exploration: &Exploration) -> Share {
        let mut share = Share::default();
        for answer in &exploration.answers {
            let recommends = answer.question.as_ref().is_some_and(|question| {
                question
                    .alternatives
                    .iter()
                    .any(|alternative| alternative.recommendation.is_some())
            });
            if recommends {
                share.add(
                    answer
                        .option
                        .as_ref()
                        .is_none_or(|option| option.recommendation.is_none()),
                );
            }
        }
        share
    }

    fn agent_turns(exploration: &Exploration) -> Vec<AgentTurn> {
        exploration
            .agent_elapsed_ms
            .iter()
            .map(|(request, &milliseconds)| AgentTurn {
                milliseconds,
                after_answer: exploration
                    .conversation
                    .iter()
                    .find(|turn| &turn.update.request == request)
                    .is_some_and(|turn| turn.answer.is_some()),
            })
            .collect()
    }

    /// How long the reviewer took to answer: from the end of the agent turn
    /// the answer replies to, to the start of the turn that carried it.
    fn reviewer_answer(round: &ExploreRound, answer: &ReviewerAnswer) -> Option<u64> {
        let asked = round.turns.get(&answer.in_reply_to)?.started_at_ms?
            + round
                .exploration
                .agent_elapsed_ms
                .get(&answer.in_reply_to)?;
        let sent = round
            .turns
            .values()
            .find(|turn| {
                turn.request
                    .answer
                    .as_ref()
                    .is_some_and(|carried| carried.id == answer.id)
            })?
            .started_at_ms?;
        sent.checked_sub(asked)
    }

    /// The words of the question and of the reply the same turn put above it.
    fn question_words(exploration: &Exploration, question: &Question) -> usize {
        let reply = exploration
            .conversation
            .iter()
            .find(|turn| turn.update.next.as_ref() == Some(question))
            .and_then(|turn| turn.update.reply.as_ref());
        reply.words() + question.words()
    }
}
