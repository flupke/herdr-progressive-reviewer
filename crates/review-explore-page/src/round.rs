//! What the page shows of an Explore round, as the session that owns the round publishes it.

use std::path::Path;
use std::sync::Arc;

use review_explore::{
    CodeLocation, Conclusion, ConversationTurn, Design, Exploration, Interpretation,
    InterviewUpdate, KeptAnswer, MarkCounts, NotRelevantMark, Question, QuizAnswers, QuizResponse,
    RoundOverview, StartBlock, Step, StepState,
};
use review_explore_citations::Citation;
use review_explore_tally::MarkTally;
use review_repository::repository::SnapshotIdentity;
use review_types::ReviewUnit;
use serde::Serialize;
use tokio::sync::watch;
use ts_rs::TS;

use crate::blind::BlindQuestion;
use crate::{CommandSender, PageConversation, PageQuizResponse, Token, Waiting};

/// The step of a round that the page shows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RoundStage {
    /// No round is running: none was started, or the reviewer reset it. `start` is the
    /// identity of the start the stage offers, which a Start from the page carries back, so that
    /// a late repeat of an earlier start starts nothing.
    NoRound { start: String },
    /// The reviewer started a round, the start `start`, which is starting: the tool captures the
    /// change, and Jev marks first when it is enabled. Then the agent works on its first turn.
    Starting {
        start: String,
        /// When the reviewer started it, in milliseconds since the epoch, if known.
        started_at_ms: Option<u64>,
    },
    /// No round is running: the reviewer's latest start failed, for this reason. `start` is the
    /// identity of the next start the stage offers.
    StartFailed { failure: String, start: String },
    /// The agent works on its next turn, the turn `request`. The reviewer may stop waiting for
    /// it.
    AgentWorking {
        request: String,
        /// When the turn's latest attempt went out to the agent, in milliseconds since the
        /// epoch; `None` while it waits to go out.
        sent_at_ms: Option<u64>,
        /// The reviewer's answer that the turn carries; `None` for a turn that carries none,
        /// such as the kickoff.
        answer: Option<Box<SentAnswer>>,
    },
    /// The agent's question, waiting for the reviewer's answer.
    Question {
        /// The question's position in the round, from 1.
        number: usize,
        question: Box<Question>,
        /// The question's citations, in the order the agent gave them.
        citations: Arc<[Citation]>,
        /// The lines an answer to the question marks.
        marks: QuestionMarks,
        /// What the agent's turn that posted the question said back to the previous answer.
        response: TurnResponse,
        /// Whether the question waits again because the reviewer cancelled its answer: the
        /// reviewer has seen the agent's recommendation already.
        answer_cancelled: bool,
    },
    /// The agent is not working on the turn the round waits for: its prompt failed, the
    /// reviewer stopped waiting, or the reviewer reopened during the turn. The reviewer may
    /// retry it.
    Interrupted {
        /// The turn that Retry sends again; `None` when the round has no turn to send again,
        /// and only Reset is left.
        request: Option<String>,
        /// The turn's latest attempt to reach the agent, which a Retry from the page carries
        /// back, so that a repeat of a Retry that went through is not a second one.
        attempt: Option<String>,
        interruption: Interruption,
        /// The reviewer's answer that the turn carries; `None` for a turn that carries none.
        answer: Option<Box<SentAnswer>>,
    },
    /// The agent concluded the round.
    Conclusion {
        /// The request of the agent's turn that posted the conclusion.
        request: String,
        conclusion: Box<Conclusion>,
        /// The latest implementation request the reviewer authorized for the conclusion, from
        /// the pane or from a page; `None` before the first.
        implementation: Option<PageImplementation>,
        /// The lines of the proofs of the conclusion's quiz, and what the reviewer answered.
        quiz: PageQuiz,
        /// What the agent's turn that posted the conclusion said back to the previous answer.
        response: TurnResponse,
    },
    /// The review tool cannot save the reviewer's rounds, for this reason: the page can do
    /// nothing until the review pane opens again.
    StorageFailed { failure: String },
}

/// The reviewer's answer that an agent's turn carries, as the page shows it beside the turn
/// while the agent works on it, or while the turn waits for Retry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SentAnswer {
    /// The question the answer answers, as the reviewer answered it; `None` for a reply to the
    /// conclusion.
    pub question: Option<AnsweredQuestion>,
    /// The choice and the comment the reviewer sent, and how the choice relates to the
    /// reviewer's first pick and to the agent's recommendation.
    pub kept: KeptAnswer,
    /// The review marks the answer applied when the reviewer sent it.
    pub marked: MarkCounts,
}

/// The question a sent answer answers, with its citations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnsweredQuestion {
    pub question: Box<Question>,
    /// The question's citations, in the order the agent gave them.
    pub citations: Arc<[Citation]>,
    /// Whether the reviewer picked a choice before the agent's recommendation showed: the
    /// question was a blind pick.
    pub picked_blind: bool,
}

/// Why the agent is not working on the turn the round waits for.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Interruption {
    /// The turn's prompt could not be delivered, for this reason.
    Failed(String),
    /// The turn's prompt reached the agent's pane, and the agent did not start on it: it may
    /// still wait in the agent's prompt box.
    NotStarted,
    /// The turn's prompt was being delivered when the reviewer reopened: whether the agent
    /// received it is unknown.
    Uncertain,
    /// The reviewer stopped waiting, or reopened before the prompt was sent.
    Stopped,
}

impl RoundStage {
    /// The question the stage waits for an answer to, when it is version `version` of `id`.
    pub(crate) fn asks(&self, id: &str, version: u32) -> Option<&Question> {
        self.asked_question()
            .filter(|question| question.is_version(id, version))
    }

    /// The question the stage waits for an answer to, whatever its version.
    pub(crate) fn asked_question(&self) -> Option<&Question> {
        match self {
            Self::Question { question, .. } => Some(question),
            _ => None,
        }
    }

    /// The question the stage waits for an answer to, when it hides the agent's recommendation
    /// until the reviewer's first pick. A question the reviewer answered before, then cancelled
    /// the answer of, shows the recommendation at once: the reviewer has seen it.
    pub(crate) fn blind(&self) -> Option<BlindQuestion<'_>> {
        match self {
            Self::Question {
                question,
                answer_cancelled: false,
                ..
            } => BlindQuestion::of(question),
            _ => None,
        }
    }

    /// What the agent's turn said back to the reviewer's previous answer, above the question or
    /// the conclusion the stage shows; `None` in another stage, or when it said nothing.
    pub fn response(&self) -> Option<&TurnResponse> {
        match self {
            Self::Question { response, .. } | Self::Conclusion { response, .. } => {
                Some(response).filter(|response| !response.is_empty())
            }
            _ => None,
        }
    }

    /// The reviewer's answer that the agent's turn carries, while the agent works on the turn or
    /// the turn waits for Retry.
    pub(crate) fn sent(&self) -> Option<&SentAnswer> {
        match self {
            Self::AgentWorking { answer, .. } | Self::Interrupted { answer, .. } => {
                answer.as_deref()
            }
            _ => None,
        }
    }

    /// No round is running or starting: the start the stage offers, by its identity.
    pub(crate) fn offered_start(&self) -> Option<&str> {
        match self {
            Self::NoRound { start } | Self::StartFailed { start, .. } => Some(start),
            _ => None,
        }
    }

    /// Whether the stage waits for what Stop waiting with `waiting` stops: the start or the
    /// agent's turn it names.
    pub(crate) fn stops(&self, waiting: &Waiting) -> bool {
        match (self, waiting) {
            (Self::Starting { start, .. }, Waiting::Start(stopped)) => start == stopped,
            (Self::AgentWorking { request, .. }, Waiting::Turn(stopped)) => request == stopped,
            _ => false,
        }
    }

    /// Whether the stage offers Retry of the attempt `attempt` of the agent's turn `request`.
    pub(crate) fn retries(&self, request: &str, attempt: &str) -> bool {
        matches!(
            self,
            Self::Interrupted { request: Some(retried), attempt: Some(latest), .. }
                if retried == request && latest == attempt
        )
    }

    /// Whether the stage shows the conclusion of the turn `conclusion`, to which the reviewer
    /// may reply.
    pub(crate) fn concludes(&self, conclusion: &str) -> bool {
        matches!(self, Self::Conclusion { request, .. } if request == conclusion)
    }

    /// Whether the stage shows the implementation request `delivery` of the conclusion as being
    /// sent: the reviewer may cancel it.
    pub(crate) fn sends_implementation(&self, delivery: &str) -> bool {
        matches!(
            self,
            Self::Conclusion { implementation: Some(implementation), .. }
                if implementation.delivery == delivery
                    && implementation.state == ImplementationState::Sending
        )
    }

    /// Whether the stage offers to send again, as it is, the attempt `attempt` of the
    /// implementation request `delivery` of the conclusion of the turn `conclusion`: it was saved
    /// but not sent, or the agent did not start on it.
    pub(crate) fn resends_implementation(
        &self,
        conclusion: &str,
        delivery: &str,
        attempt: &str,
    ) -> bool {
        matches!(
            self,
            Self::Conclusion { request, implementation: Some(implementation), .. }
                if request == conclusion
                    && implementation.delivery == delivery
                    && implementation.attempt == attempt
                    && matches!(
                        implementation.state,
                        ImplementationState::Paused | ImplementationState::NotStarted
                    )
        )
    }

    /// Whether the stage shows the quiz `response` answers, where it fits, as the round would
    /// record it: a pick of the item the quiz asks next, or of an item again with the same
    /// option, or a skip.
    pub(crate) fn takes_quiz(&self, response: &PageQuizResponse) -> bool {
        let Self::Conclusion {
            request,
            conclusion,
            quiz,
            ..
        } = self
        else {
            return false;
        };
        *request == response.conclusion
            && quiz.takes_answers
            && quiz
                .answers
                .clone()
                .record(&conclusion.quiz, response.response)
                .is_ok()
    }

    /// The latest implementation request of the conclusion the stage shows, if any.
    pub(crate) fn implementation(&self) -> Option<&PageImplementation> {
        match self {
            Self::Conclusion { implementation, .. } => implementation.as_ref(),
            _ => None,
        }
    }

    /// Whether the conclusion's quiz has `response` saved already: the same pick of an item, or
    /// a skip.
    pub(crate) fn quiz_has(&self, response: &PageQuizResponse) -> bool {
        let Self::Conclusion { request, quiz, .. } = self else {
            return false;
        };
        *request == response.conclusion
            && match response.response {
                QuizResponse::Pick { item, answer } => quiz
                    .answers
                    .pick(item)
                    .is_some_and(|pick| pick.answer == answer),
                QuizResponse::Skip => quiz.answers.skipped,
            }
    }

    /// Whether the stage offers Implement for the conclusion of the turn `conclusion`, in place
    /// of its request `replaces`, which the agent did not receive or may not have received, or
    /// as its first request when `replaces` is `None`.
    pub(crate) fn offers_implement(&self, conclusion: &str, replaces: Option<&str>) -> bool {
        let Self::Conclusion {
            request,
            implementation,
            ..
        } = self
        else {
            return false;
        };
        request == conclusion
            && match implementation {
                None => replaces.is_none(),
                Some(implementation) => {
                    implementation.state.allows_another()
                        && replaces == Some(implementation.delivery.as_str())
                }
            }
    }
}

/// An implementation request the reviewer authorized for a conclusion, and what became of it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PageImplementation {
    /// The request's delivery identity.
    pub delivery: String,
    /// Its latest attempt to reach the agent.
    pub attempt: String,
    /// The list to be implemented that the request sends.
    pub text: String,
    pub state: ImplementationState,
    /// When the agent received it, in milliseconds since the epoch; `None` until then, and for
    /// a request saved before rounds kept the time.
    pub sent_at_ms: Option<u64>,
}

impl PageImplementation {
    /// How many items the request's list has.
    pub(crate) fn items(&self) -> usize {
        list_items(&self.text)
    }
}

/// How many items the list to be implemented `text` has: its lines that are not blank, as the
/// page counts the list the reviewer edits (assets/client/conclusion.js).
pub(crate) fn list_items(text: &str) -> usize {
    text.lines().filter(|line| !line.trim().is_empty()).count()
}

/// What the page shows of a conclusion's quiz beside its items: the lines of each item's proof,
/// and what the reviewer answered. A conclusion without a quiz has neither.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PageQuiz {
    /// The lines of each item's proof, in the order of the quiz.
    pub proofs: Vec<Arc<[Citation]>>,
    pub answers: QuizAnswers,
    /// Whether the round can save the reviewer's answers. When it cannot (an earlier round, or
    /// a storage failure), the page asks nothing and shows the conclusion.
    pub takes_answers: bool,
}

/// What an agent's turn said back to the reviewer's previous answer, as the pane shows it: its
/// interpretations of the answer, each with its recap and follow-ups, and its reply, and whether
/// run-ahead prepared the turn. Empty for a turn that follows no answer, replies nothing and was
/// not prepared.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct TurnResponse {
    /// The agent's interpretations of the answer the turn follows, in the order it gave them.
    pub interpretations: Vec<Interpretation>,
    /// The agent's reply, in Markdown.
    pub reply: Option<String>,
    /// Whether a fork took the turn while the reviewer thought about the answer, and the agent
    /// continued as that fork.
    pub prepared: bool,
}

impl TurnResponse {
    /// What `turn` of `exploration` said back: every interpretation of the answer the turn
    /// follows, as the pane lists them under the answer, and the turn's reply.
    pub fn of(exploration: &Exploration, turn: &ConversationTurn) -> Self {
        Self {
            interpretations: turn.answer.as_deref().map_or_else(Vec::new, |answer| {
                exploration
                    .interpretations
                    .iter()
                    .filter(|interpretation| interpretation.answer == answer)
                    .cloned()
                    .collect()
            }),
            reply: turn
                .update
                .reply
                .as_ref()
                .map(|reply| reply.text.clone())
                .filter(|text| !text.trim().is_empty()),
            prepared: false,
        }
    }

    fn is_empty(&self) -> bool {
        self.interpretations.is_empty() && self.reply.is_none() && !self.prepared
    }
}

/// The review a page belongs to, as the pane's header names it, with the repository: two
/// reviewers' pages can be told apart.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, TS)]
pub struct ReviewName {
    /// The name of the repository's directory.
    pub repository: String,
    /// The abbreviated revision identifier, without colors.
    pub revision: String,
    /// The first line of the change's description; empty when it has none.
    pub title: String,
}

impl ReviewName {
    /// The review of the repository at `root`, at the snapshot `identity`.
    pub fn of(root: &Path, identity: &SnapshotIdentity) -> Self {
        Self {
            repository: root.file_name().map_or_else(
                || root.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            ),
            revision: identity.plain_display_id(),
            title: identity.title().to_owned(),
        }
    }
}

/// What became of an implementation request, as the page shows it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, TS)]
#[serde(tag = "kind", content = "reason", rename_all = "snake_case")]
pub enum ImplementationState {
    /// The reviewer sends it to the agent now.
    Sending,
    /// The agent received it.
    Sent,
    /// It was saved, then not sent: the reviewer reopened first. The reviewer may send it, or
    /// a new one.
    Paused,
    /// Whether the agent received it is unknown. The reviewer may send a new one, after
    /// checking the agent's pane.
    Unknown,
    /// The agent did not start on it: the text may still wait in the agent's prompt box. The
    /// reviewer looks at the agent's pane, then sends it again as it is.
    NotStarted,
    /// It could not be sent, for this reason. The reviewer may send another.
    NotSent(String),
    /// The reviewer cancelled it before it was sent, and may send another.
    Cancelled,
}

impl ImplementationState {
    /// Whether the reviewer may send another request in place of this one: the request is
    /// neither on its way nor received by the agent, and does not wait in the agent's prompt
    /// box, where a request the agent did not start on may still be.
    pub fn allows_another(&self) -> bool {
        matches!(
            self,
            Self::Paused | Self::Unknown | Self::NotSent(_) | Self::Cancelled
        )
    }
}

/// The lines that an answer to a question marks reviewed, marks not relevant and reopens, as
/// the agent's turn that posted the question asked.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct QuestionMarks {
    pub reviewed: Vec<CodeLocation>,
    pub not_relevant: Vec<NotRelevantMark>,
    pub reopened: Vec<CodeLocation>,
}

impl QuestionMarks {
    /// What `turn` asks to mark once its question has an answer.
    pub fn requested(turn: &InterviewUpdate) -> Self {
        Self {
            reviewed: turn.reviewed.clone(),
            not_relevant: turn.not_relevant.clone(),
            reopened: turn.reopened.clone(),
        }
    }

    /// How many lines and whole files the marks mark reviewed, mark not relevant and reopen.
    pub fn counts(&self) -> MarkCounts {
        MarkCounts::count(
            &self.reviewed,
            NotRelevantMark::locations(&self.not_relevant),
            &self.reopened,
        )
    }
}

/// A running round, as its owner publishes it beside its stage.
#[derive(Clone, Copy, Debug)]
pub struct PublishedRound<'a> {
    /// The round's identity.
    pub id: &'a str,
    /// The review the round belongs to, whose threads hold the round's conversation.
    pub review_unit: &'a ReviewUnit,
    /// The design of the change, once the round's first turn explained it.
    pub design: Option<&'a Design>,
    /// How many files the round's change touches, which the design's meta line counts.
    pub changed_files: usize,
    /// The reviewer's latest answer, while the reviewer may cancel it.
    pub cancellable: Option<&'a LatestAnswer>,
    /// Whether the round is an earlier one: a newer round of the review was saved since, by
    /// another reviewer. The reviewer can only Reset it.
    pub earlier: bool,
    /// Where the round stands as a whole: its rail and the tab title.
    pub overview: &'a RoundOverview,
    /// The citations of each earlier question of the overview, in the same order, with the
    /// lines of the change they name.
    pub earlier_citations: &'a [Arc<[Citation]>],
}

/// The reviewer's latest answer of a round, which the page offers to cancel as the pane does.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, TS)]
pub struct LatestAnswer {
    /// The answer's identity.
    pub id: String,
    /// The text of the choice the reviewer picked, if any.
    pub choice: Option<String>,
    /// The reviewer's comment; empty when there is none.
    pub comment: String,
    /// What the answer answered, so that the page knows a repeat of it.
    #[serde(skip)]
    #[ts(skip)]
    pub answered: Answered,
}

/// What an answer answered, and how: a question, or the conclusion of a turn.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Answered {
    /// The question and its version; `None` for a reply to the conclusion.
    pub question: Option<(String, u32)>,
    /// The picked choice's ID, if any.
    pub option: Option<String>,
    /// The request of the agent's turn the answer replied to.
    pub in_reply_to: String,
}

/// A stage, and a revision that changes with every published stage. A page that shows the
/// agent working loads itself again once the revision changes.
#[derive(Clone, Debug)]
pub(crate) struct RoundSnapshot {
    pub(crate) revision: u64,
    /// The identity of the round the stage belongs to; `None` when no round is running.
    pub(crate) round: Option<String>,
    /// The review the round belongs to; `None` when no round is running.
    pub(crate) review_unit: Option<ReviewUnit>,
    /// The design of the change, as the round's first turn explained it.
    pub(crate) design: Option<Arc<Design>>,
    /// How many files the round's change touches.
    pub(crate) changed_files: usize,
    /// The reviewer's latest answer, while the reviewer may cancel it.
    pub(crate) cancellable: Option<LatestAnswer>,
    /// Whether the round is an earlier one, which the reviewer can only Reset.
    pub(crate) earlier: bool,
    /// Where the round stands as a whole: its rail and the tab title.
    pub(crate) overview: Option<Arc<RoundOverview>>,
    /// The citations of each earlier question of the overview, in the same order.
    pub(crate) earlier_citations: Arc<[Arc<[Citation]>]>,
    pub(crate) stage: RoundStage,
    /// The review the page belongs to, once its owner named it.
    pub(crate) review: Option<ReviewName>,
    /// Why the reviewer cannot start a round on the review, when nothing is left to review.
    pub(crate) start_block: Option<StartBlock>,
    /// How much of the change the review marks cover, once the owner counted them.
    pub(crate) tally: Option<Arc<MarkTally>>,
}

impl RoundSnapshot {
    /// The number of the question the stage asks, by the round's overview: a clarified
    /// question keeps its step's number on the rail, where `RoundStage::Question` counts every
    /// version, which stands in only for a round with no overview. `None` when the stage asks
    /// no question.
    pub(crate) fn question_number(&self) -> Option<usize> {
        let RoundStage::Question { number, .. } = self.stage else {
            return None;
        };
        let step = self.overview.as_deref().and_then(|overview| {
            overview
                .rail
                .iter()
                .find_map(|step| match (&step.step, step.state) {
                    (Step::Question { number }, StepState::Current { .. }) => Some(*number),
                    _ => None,
                })
        });
        Some(step.unwrap_or(number))
    }

    /// The number on the rail of the question that the reviewer's latest answer, which the
    /// reviewer may still cancel, answered. `None` for a reply to the conclusion, and for a round
    /// with no overview.
    pub(crate) fn answered_number(&self) -> Option<usize> {
        self.cancellable.as_ref()?.answered.question.as_ref()?;
        self.latest_answer_number()
    }

    /// The number on the rail of the question that the answer the agent's turn carries
    /// answered. `None` when the turn carries no answer to a question, and for a round with no
    /// overview.
    pub(crate) fn sent_number(&self) -> Option<usize> {
        self.stage.sent()?.question.as_ref()?;
        self.latest_answer_number()
    }

    /// The number on the rail of the question the reviewer's latest answer answered: the step
    /// before the question or the conclusion the answer led to, or the step the agent works on,
    /// or waits for a Retry of.
    fn latest_answer_number(&self) -> Option<usize> {
        let rail = &self.overview.as_deref()?.rail;
        let mut done = None;
        let mut current = None;
        for step in rail {
            match (&step.step, step.state) {
                (Step::Question { number }, StepState::Done) => done = Some(*number),
                (Step::Question { number }, StepState::Current { .. }) => current = Some(*number),
                _ => {}
            }
        }
        match self.stage {
            RoundStage::Question { .. } | RoundStage::Conclusion { .. } => done,
            _ => current.or(done),
        }
    }

    /// The round whose conversation the page offers, by its instance, with the review whose
    /// threads hold it: `None` when no round is running, and when the review tool cannot save
    /// the round.
    pub(crate) fn conversation_round(&self) -> Option<(&str, &ReviewUnit)> {
        // No round runs on the start cover: there is nobody to talk to yet.
        if matches!(
            self.stage,
            RoundStage::StorageFailed { .. }
                | RoundStage::NoRound { .. }
                | RoundStage::StartFailed { .. }
        ) {
            return None;
        }
        self.round.as_deref().zip(self.review_unit.as_ref())
    }

    /// The number on the rail of version `version` of the question `id`: a question the round
    /// went past, the one the stage asks, or the one the reviewer's latest answer answered. A
    /// question asked again after another has a step of its own, so the version decides; an
    /// earlier version that a clarification followed has its clarification's step. `None` for
    /// another question, and for a round with no overview.
    pub(crate) fn number_of_question(&self, id: &str, version: u32) -> Option<usize> {
        self.numbered_version(id, Some(version))
            .or_else(|| self.numbered_version(id, None))
    }

    /// The number on the rail of the question `id` in version `version`, or in any version.
    fn numbered_version(&self, id: &str, version: Option<u32>) -> Option<usize> {
        let is = |question_id: &str, question_version: u32| {
            question_id == id && version.is_none_or(|version| version == question_version)
        };
        let overview = self.overview.as_deref()?;
        if let Some(earlier) = overview
            .earlier
            .iter()
            .rev()
            .find(|earlier| is(&earlier.question.id, earlier.question.version))
        {
            return Some(earlier.number);
        }
        if self
            .stage
            .asked_question()
            .is_some_and(|asked| is(&asked.id, asked.version))
        {
            return self.question_number();
        }
        let answered = self.cancellable.as_ref()?.answered.question.as_ref()?;
        is(&answered.0, answered.1)
            .then(|| self.answered_number())
            .flatten()
    }

    /// Whether the round's latest answer, which the reviewer may still cancel, is `answered`
    /// with the comment `comment`: a repeat of an answer or a reply that went through.
    pub(crate) fn repeats(&self, answered: &Answered, comment: &str) -> bool {
        self.cancellable.as_ref().is_some_and(|latest| {
            let same = match &answered.question {
                Some(_) => {
                    latest.answered.question == answered.question
                        && latest.answered.option == answered.option
                }
                None => {
                    latest.answered.question.is_none()
                        && latest.answered.option.is_none()
                        && latest.answered.in_reply_to == answered.in_reply_to
                }
            };
            same && latest.comment == comment
        })
    }

    fn shows(&self, round: Option<PublishedRound<'_>>, stage: &RoundStage) -> bool {
        self.stage == *stage
            && self.round.as_deref() == round.map(|round| round.id)
            && self.review_unit.as_ref() == round.map(|round| round.review_unit)
            && self.design.as_deref() == round.and_then(|round| round.design)
            && self.changed_files == round.map_or(0, |round| round.changed_files)
            && self.cancellable.as_ref() == round.and_then(|round| round.cancellable)
            && self.earlier == round.is_some_and(|round| round.earlier)
            && self.overview.as_deref() == round.map(|round| round.overview)
            && *self.earlier_citations == *round.map_or(&[][..], |round| round.earlier_citations)
    }

    /// The snapshot of `stage` of the round `round`, at `revision`, of the review `review`,
    /// whose start `start_block` blocks.
    fn new(
        revision: u64,
        round: Option<PublishedRound<'_>>,
        stage: RoundStage,
        review: Option<ReviewName>,
        start_block: Option<StartBlock>,
    ) -> Self {
        Self {
            revision,
            round: round.map(|round| round.id.to_owned()),
            review_unit: round.map(|round| round.review_unit.clone()),
            design: round.and_then(|round| round.design.cloned().map(Arc::new)),
            changed_files: round.map_or(0, |round| round.changed_files),
            cancellable: round.and_then(|round| round.cancellable.cloned()),
            earlier: round.is_some_and(|round| round.earlier),
            overview: round.map(|round| Arc::new(round.overview.clone())),
            earlier_citations: round
                .map_or_else(|| Arc::from([]), |round| round.earlier_citations.into()),
            stage,
            review,
            start_block,
            tally: None,
        }
    }
}

/// The owner's side of one round: publishes its stage to every page that shows it.
pub struct RoundPublisher(watch::Sender<RoundSnapshot>);

impl RoundPublisher {
    /// A publisher whose first stage is `stage` of the round `round`, `None` when no round is
    /// running.
    pub fn new(round: Option<PublishedRound<'_>>, stage: RoundStage) -> Self {
        Self(watch::Sender::new(RoundSnapshot::new(
            1, round, stage, None, None,
        )))
    }

    /// Publishes `stage` of the round `round`, `None` when no round is running. The same stage
    /// of the same round again changes nothing, so a page that waits for the agent does not
    /// load itself again for nothing.
    pub fn publish(&self, round: Option<PublishedRound<'_>>, stage: RoundStage) {
        self.publish_with(round, stage, None);
    }

    /// Publishes `stage` of the round `round` as [`Self::publish`] does, with how much of the
    /// change the review marks cover as `tally` says, in one change: the page never shows the
    /// marks an answer applied beside the question that still waits for it.
    pub fn publish_counted(
        &self,
        round: Option<PublishedRound<'_>>,
        stage: RoundStage,
        tally: MarkTally,
    ) {
        self.publish_with(round, stage, Some(tally));
    }

    fn publish_with(
        &self,
        round: Option<PublishedRound<'_>>,
        stage: RoundStage,
        tally: Option<MarkTally>,
    ) {
        self.0.send_if_modified(|snapshot| {
            let tally = tally
                .filter(|tally| snapshot.tally.as_deref() != Some(tally))
                .map(Arc::new);
            // The page shows this stage already: only new marks change it.
            let shown = snapshot.shows(round, &stage);
            if shown && tally.is_none() {
                return false;
            }
            if shown {
                snapshot.revision += 1;
            } else {
                let kept = snapshot.tally.take();
                *snapshot = RoundSnapshot {
                    tally: kept,
                    ..RoundSnapshot::new(
                        snapshot.revision + 1,
                        round,
                        stage,
                        snapshot.review.take(),
                        snapshot.start_block,
                    )
                };
            }
            if tally.is_some() {
                snapshot.tally = tally;
            }
            true
        });
    }

    /// Names the review the page belongs to. The same name again changes nothing.
    pub fn name(&self, review: ReviewName) {
        self.0.send_if_modified(|snapshot| {
            if snapshot.review.as_ref() == Some(&review) {
                return false;
            }
            snapshot.revision += 1;
            snapshot.review = Some(review);
            true
        });
    }

    /// Says why the reviewer cannot start a round on the review, or, with `None`, that a round
    /// can start. The same again changes nothing.
    pub fn block_starts(&self, block: Option<StartBlock>) {
        self.0.send_if_modified(|snapshot| {
            if snapshot.start_block == block {
                return false;
            }
            snapshot.revision += 1;
            snapshot.start_block = block;
            true
        });
    }

    /// Publishes how much of the change the review marks cover, as they stand now. The same
    /// tally again changes nothing.
    pub fn tally(&self, tally: MarkTally) {
        self.0.send_if_modified(|snapshot| {
            if snapshot.tally.as_deref() == Some(&tally) {
                return false;
            }
            snapshot.revision += 1;
            snapshot.tally = Some(Arc::new(tally));
            true
        });
    }

    pub fn subscribe(&self) -> RoundFeed {
        RoundFeed(self.0.subscribe())
    }
}

/// A round no one started yet.
impl Default for RoundPublisher {
    fn default() -> Self {
        let start = uuid::Uuid::new_v4().simple().to_string();
        Self::new(None, RoundStage::NoRound { start })
    }
}

/// The page's side of one round: its latest published stage.
#[derive(Clone, Debug)]
pub struct RoundFeed(watch::Receiver<RoundSnapshot>);

impl RoundFeed {
    /// The latest published stage.
    pub fn stage(&self) -> RoundStage {
        self.0.borrow().stage.clone()
    }

    /// The identity of the round the latest stage belongs to; `None` when no round is running.
    pub fn round(&self) -> Option<String> {
        self.0.borrow().round.clone()
    }

    /// The design of the change, as the latest round's first turn explained it.
    pub fn design(&self) -> Option<Arc<Design>> {
        self.0.borrow().design.clone()
    }

    /// The reviewer's latest answer of the latest round, while the reviewer may cancel it.
    pub fn cancellable(&self) -> Option<LatestAnswer> {
        self.0.borrow().cancellable.clone()
    }

    /// Whether the latest round is an earlier one, which the reviewer can only Reset.
    pub fn earlier(&self) -> bool {
        self.0.borrow().earlier
    }

    /// Where the latest round stands as a whole; `None` when no round is running.
    pub fn overview(&self) -> Option<Arc<RoundOverview>> {
        self.0.borrow().overview.clone()
    }

    /// The citations of each earlier question of the overview, in the same order.
    pub fn earlier_citations(&self) -> Arc<[Arc<[Citation]>]> {
        self.0.borrow().earlier_citations.clone()
    }

    /// The review the page belongs to, once the owner named it.
    pub fn review(&self) -> Option<ReviewName> {
        self.0.borrow().review.clone()
    }

    /// Why the reviewer cannot start a round on the review; `None` when a round can start.
    pub fn start_block(&self) -> Option<StartBlock> {
        self.0.borrow().start_block
    }

    /// How much of the change the review marks cover; `None` until the owner counted them.
    pub fn tally(&self) -> Option<Arc<MarkTally>> {
        self.0.borrow().tally.clone()
    }

    /// Waits for the next published stage. Returns false once the publisher is gone.
    pub async fn changed(&mut self) -> bool {
        self.0.changed().await.is_ok()
    }

    pub(crate) fn latest(&self) -> RoundSnapshot {
        self.0.borrow().clone()
    }
}

/// A round as a page sees it: the stages its owner publishes, and where the reviewer's
/// commands for it go.
#[derive(Clone)]
pub struct PageRound {
    pub(crate) stages: RoundFeed,
    pub(crate) commands: CommandSender,
    /// The review threads, which hold the round's conversation; `None` when the page offers no
    /// conversation.
    pub(crate) conversation: Option<PageConversation>,
}

impl PageRound {
    pub fn new(stages: RoundFeed, commands: CommandSender) -> Self {
        Self {
            stages,
            commands,
            conversation: None,
        }
    }

    /// The round, whose conversation the page reads from and writes to `conversation`.
    #[must_use]
    pub fn with_conversation(mut self, conversation: PageConversation) -> Self {
        self.conversation = Some(conversation);
        self
    }

    /// The stages the round's owner publishes.
    pub fn stages(&self) -> &RoundFeed {
        &self.stages
    }
}

/// The rounds a page can show, each behind the token of the address that opens it.
pub trait Rounds: Send + Sync + 'static {
    /// The round that `token` opens, or `None` when no round has that token. Compare tokens
    /// with [`Token::matches`](crate::Token::matches).
    fn find(&self, token: &str) -> Option<PageRound>;

    /// The token that opens the page once the reviewer reset the round from the page: `None`
    /// when the token that opened the round opens the page still, or when no token opens its
    /// start screen any more because the next round started already.
    fn after_reset(&self) -> Option<Token> {
        None
    }
}
