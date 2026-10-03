//! What the page shows of an Explore round, as the session that owns the round publishes it.

use review_explore::Question;
use tokio::sync::watch;

/// The step of a round that the page shows.
#[derive(Clone, Debug)]
pub enum RoundStage {
    /// The agent works on its next question.
    AgentWorking,
    /// The agent's question, waiting for the reviewer's answer.
    Question {
        /// The question's position in the round, from 1.
        number: usize,
        question: Box<Question>,
    },
}

/// A stage, and a revision that changes with every published stage. A page that shows the
/// agent working loads itself again once the revision changes.
#[derive(Clone, Debug)]
pub(crate) struct RoundSnapshot {
    pub(crate) revision: u64,
    pub(crate) stage: RoundStage,
}

/// The owner's side of one round: publishes its stage to every page that shows it.
pub struct RoundPublisher(watch::Sender<RoundSnapshot>);

impl RoundPublisher {
    pub fn new(stage: RoundStage) -> Self {
        Self(watch::Sender::new(RoundSnapshot { revision: 1, stage }))
    }

    pub fn publish(&self, stage: RoundStage) {
        self.0.send_modify(|snapshot| {
            snapshot.revision += 1;
            snapshot.stage = stage;
        });
    }

    pub fn subscribe(&self) -> RoundFeed {
        RoundFeed(self.0.subscribe())
    }
}

/// The page's side of one round: its latest published stage.
#[derive(Clone, Debug)]
pub struct RoundFeed(watch::Receiver<RoundSnapshot>);

impl RoundFeed {
    pub(crate) fn latest(&self) -> RoundSnapshot {
        self.0.borrow().clone()
    }
}

/// The rounds a page can show, each behind the token of the address that opens it.
pub trait Rounds: Send + Sync + 'static {
    /// The round that `token` opens, or `None` when no round has that token. Compare tokens
    /// with [`Token::matches`](crate::Token::matches).
    fn find(&self, token: &str) -> Option<RoundFeed>;
}
