//! The round's overview. The review tool derives it from the saved round
//! (`review_explore::RoundOverview::of`); the standalone server saves no round, so it tells the
//! same rules where the round stands (`review_explore::RoundStanding`): what its agent posted
//! so far and the stage the round is at. Its questions are never clarified, and an answer is
//! cancelled only on the latest question, so the steps are the questions asked.

use review_explore::{Activity, LatestTurn, QuizProgress, RoundOverview, RoundStanding};
use review_explore_page::RoundStage;

/// What the agent of a session posted so far in the latest round.
pub(crate) struct Posted<'a> {
    /// The questions the agent asked.
    pub(crate) asked: usize,
    /// Whether the agent's latest turn posted the conclusion.
    pub(crate) concluded: bool,
    /// The conclusion's quiz, when it has one.
    pub(crate) quiz: Option<QuizProgress<'a>>,
}

impl Posted<'_> {
    /// The overview of the round at `stage`.
    pub(crate) fn overview(&self, stage: &RoundStage) -> RoundOverview {
        let activity = match stage {
            RoundStage::AgentWorking { .. } => Activity::Working,
            RoundStage::Interrupted { .. } => Activity::Interrupted,
            _ => Activity::Idle,
        };
        let latest = if self.concluded {
            LatestTurn::Conclusion { quiz: self.quiz }
        } else if self.asked > 0 {
            LatestTurn::Question
        } else {
            LatestTurn::None
        };
        RoundStanding {
            // The agent's design comes with its first question.
            design: true,
            questions: self.asked,
            activity,
            latest,
        }
        .overview()
    }
}
