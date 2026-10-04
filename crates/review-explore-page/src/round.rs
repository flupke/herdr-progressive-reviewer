//! What the page shows of an Explore round, as the session that owns the round publishes it.

use std::path::Path;
use std::sync::Arc;

use axum::http::HeaderMap;
use review_explore::{
    CodeLocation, Conclusion, ConversationTurn, Design, Exploration, Interpretation,
    InterviewUpdate, MarkCounts, NotRelevantMark, Question, QuizAnswers,
};
use review_explore_citations::Citation;
use review_repository::repository::SnapshotIdentity;
use serde::Serialize;
use tokio::sync::watch;

use crate::blind::{BlindQuestion, FirstPick};
use crate::{CommandSender, PageQuizResponse};

/// The step of a round that the page shows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RoundStage {
    /// No round is running: none was started, or the reviewer reset it.
    NoRound,
    /// The reviewer started a round, which is starting: the tool captures the change, and Jev
    /// marks first when it is enabled. Then the agent works on its first turn.
    Starting,
    /// No round is running: the reviewer's latest start failed, for this reason.
    StartFailed { failure: String },
    /// The agent works on its next turn.
    AgentWorking,
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
    /// reviewer stopped waiting, or the reviewer reopened during the turn. The reviewer
    /// retries in the pane.
    Interrupted {
        /// Why the prompt of the turn could not be delivered, when it failed.
        failure: Option<String>,
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
}

impl RoundStage {
    /// The question the stage waits for an answer to, when it is version `version` of `id`.
    pub(crate) fn asks(&self, id: &str, version: u32) -> Option<&Question> {
        match self {
            Self::Question { question, .. } if question.is_version(id, version) => Some(question),
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

    /// Whether the reviewer can start a round: none is running or starting.
    pub(crate) fn can_start(&self) -> bool {
        matches!(self, Self::NoRound | Self::StartFailed { .. })
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

    /// Whether the stage offers Implement for the conclusion of the turn `conclusion`, in place
    /// of its request `replaces`, which was not sent, or as its first request when `replaces` is
    /// `None`.
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
    /// The list to be implemented that the request sends.
    pub text: String,
    pub state: ImplementationState,
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
/// interpretations of the answer, each with its recap and follow-ups, and its reply. Empty for a
/// turn that follows no answer and replies nothing.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct TurnResponse {
    /// The agent's interpretations of the answer the turn follows, in the order it gave them.
    pub interpretations: Vec<Interpretation>,
    /// The agent's reply, in Markdown.
    pub reply: Option<String>,
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
        }
    }

    fn is_empty(&self) -> bool {
        self.interpretations.is_empty() && self.reply.is_none()
    }
}

/// The review a page belongs to, as the pane's header names it, with the repository: two
/// reviewers' pages can be told apart.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
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
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "reason", rename_all = "snake_case")]
pub enum ImplementationState {
    /// The reviewer sends it to the agent now.
    Sending,
    /// The agent received it.
    Sent,
    /// It was saved, then not sent: the reviewer reopened first. The reviewer sends it, or a
    /// new one, from the pane.
    Paused,
    /// Whether the agent received it is unknown.
    Unknown,
    /// It could not be sent, for this reason. The reviewer may send another.
    NotSent(String),
    /// The reviewer cancelled it before it was sent, and may send another.
    Cancelled,
}

impl ImplementationState {
    /// Whether the reviewer may send another request: the agent cannot have received this one,
    /// as `review_explore::DispatchState::undelivered` says of the saved request.
    fn allows_another(&self) -> bool {
        matches!(self, Self::NotSent(_) | Self::Cancelled)
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

    pub(crate) fn counts(&self) -> MarkCounts {
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
    /// The design of the change, once the round's first turn explained it.
    pub design: Option<&'a Design>,
}

/// A stage, and a revision that changes with every published stage. A page that shows the
/// agent working loads itself again once the revision changes.
#[derive(Clone, Debug)]
pub(crate) struct RoundSnapshot {
    pub(crate) revision: u64,
    /// The identity of the round the stage belongs to; `None` when no round is running.
    pub(crate) round: Option<String>,
    /// The design of the change, as the round's first turn explained it.
    pub(crate) design: Option<Arc<Design>>,
    pub(crate) stage: RoundStage,
    /// The review the page belongs to, once its owner named it.
    pub(crate) review: Option<ReviewName>,
}

impl RoundSnapshot {
    /// The reviewer's first pick of the blind question the snapshot asks, as the request's
    /// cookie carries it; `None` when the question shows its recommendation at once.
    pub(crate) fn first_pick(&self, headers: &HeaderMap) -> Option<FirstPick> {
        let blind = self.stage.blind()?;
        FirstPick::read(headers, self.round.as_deref(), &blind)
    }

    fn shows(&self, round: Option<PublishedRound<'_>>, stage: &RoundStage) -> bool {
        self.stage == *stage
            && self.round.as_deref() == round.map(|round| round.id)
            && self.design.as_deref() == round.and_then(|round| round.design)
    }
}

/// The owner's side of one round: publishes its stage to every page that shows it.
pub struct RoundPublisher(watch::Sender<RoundSnapshot>);

impl RoundPublisher {
    /// A publisher whose first stage is `stage` of the round `round`, `None` when no round is
    /// running.
    pub fn new(round: Option<PublishedRound<'_>>, stage: RoundStage) -> Self {
        Self(watch::Sender::new(RoundSnapshot {
            revision: 1,
            round: round.map(|round| round.id.to_owned()),
            design: round.and_then(|round| round.design.cloned().map(Arc::new)),
            stage,
            review: None,
        }))
    }

    /// Publishes `stage` of the round `round`, `None` when no round is running. The same stage
    /// of the same round again changes nothing, so a page that waits for the agent does not
    /// load itself again for nothing.
    pub fn publish(&self, round: Option<PublishedRound<'_>>, stage: RoundStage) {
        self.0.send_if_modified(|snapshot| {
            if snapshot.shows(round, &stage) {
                return false;
            }
            *snapshot = RoundSnapshot {
                revision: snapshot.revision + 1,
                round: round.map(|round| round.id.to_owned()),
                design: round.and_then(|round| round.design.cloned().map(Arc::new)),
                stage,
                review: snapshot.review.take(),
            };
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

    pub fn subscribe(&self) -> RoundFeed {
        RoundFeed(self.0.subscribe())
    }
}

/// A round no one started yet.
impl Default for RoundPublisher {
    fn default() -> Self {
        Self::new(None, RoundStage::NoRound)
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

    /// The review the page belongs to, once the owner named it.
    pub fn review(&self) -> Option<ReviewName> {
        self.0.borrow().review.clone()
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
}

impl PageRound {
    pub fn new(stages: RoundFeed, commands: CommandSender) -> Self {
        Self { stages, commands }
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
}
