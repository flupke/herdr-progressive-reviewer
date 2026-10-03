//! What the page shows of an Explore round, as the session that owns the round publishes it.

use std::sync::Arc;

use review_explore::{
    CodeLocation, Conclusion, Design, InterviewUpdate, MarkCounts, NotRelevantMark, Question,
};
use review_explore_citations::Citation;
use tokio::sync::watch;

use crate::CommandSender;

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
    },
    /// The agent is not working on the turn the round waits for: its prompt failed, the
    /// reviewer stopped waiting, or the reviewer reopened during the turn. The reviewer
    /// retries in the pane.
    Interrupted {
        /// Why the prompt of the turn could not be delivered, when it failed.
        failure: Option<String>,
    },
    /// The agent concluded the round.
    Conclusion(Box<Conclusion>),
}

impl RoundStage {
    /// The question the stage waits for an answer to, when it is version `version` of `id`.
    pub(crate) fn asks(&self, id: &str, version: u32) -> Option<&Question> {
        match self {
            Self::Question { question, .. } if question.is_version(id, version) => Some(question),
            _ => None,
        }
    }

    /// Whether the reviewer can start a round: none is running or starting.
    pub(crate) fn can_start(&self) -> bool {
        matches!(self, Self::NoRound | Self::StartFailed { .. })
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
}

impl RoundSnapshot {
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
            };
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
