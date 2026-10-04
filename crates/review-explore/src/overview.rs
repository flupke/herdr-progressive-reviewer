//! What the Explore page shows of a whole round around its current screen: the steps of the
//! round rail, the reviewer's decisions, the record of each earlier question and the tab
//! title. Plain data derived from the saved round, for the page to draw as it is.

mod answer;
mod quiz;
mod rail;
mod record;
mod steps;
mod title;

pub use answer::{Decision, DecisionTag, KeptAnswer};
pub use quiz::{QuizProgress, QuizStage};
pub use rail::{RailStep, Step, StepState};
pub use record::{AgentRecord, EarlierQuestion};
pub use title::TabTitle;

use serde::Serialize;

use crate::ExploreRound;
pub use steps::{Activity, LatestTurn};
use steps::{Position, QuestionStep};

/// The overview of one round, derived from its saved state each time it changes.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RoundOverview {
    /// The steps of the round rail, in order.
    pub rail: Vec<RailStep>,
    /// The reviewer's decisions, one for each question whose latest version was answered, in
    /// the order of the rail.
    pub decisions: Vec<Decision>,
    /// The record of each done question step, to show read only, in the order of the rail.
    pub earlier: Vec<EarlierQuestion>,
    /// What the browser tab's title says before the review's name.
    pub title: TabTitle,
}

impl RoundOverview {
    /// The overview of `round`. `delivering` names the turn, by its request, that the session
    /// delivers to the agent now: a pending turn the session does not deliver, after a
    /// reopening or Stop waiting, waits for Retry.
    pub fn of(round: &ExploreRound, delivering: Option<&str>) -> Self {
        let exploration = &round.exploration;
        let steps = QuestionStep::of(exploration);
        let position = Position::of(exploration, &steps, delivering);
        Self {
            // A round saved before the first turn explained the design has no Design step.
            rail: RailStep::rail(
                exploration.design().is_some() || exploration.conversation.is_empty(),
                steps.len(),
                &position,
            ),
            decisions: steps
                .iter()
                .filter_map(|step| Decision::of(exploration, step))
                .collect(),
            earlier: steps
                .iter()
                .filter(|step| position.passed(step))
                .map(|step| EarlierQuestion::of(round, step))
                .collect(),
            title: TabTitle::of(&position),
        }
    }
}

/// Where a round stands, told as plain facts instead of read from its saved state: for a stand-in
/// of the Explore session that keeps no saved round, such as the standalone page server.
#[derive(Clone, Copy, Debug)]
pub struct RoundStanding<'a> {
    /// Whether the rail has a Design step.
    pub design: bool,
    /// The question steps posted so far.
    pub questions: usize,
    pub activity: Activity,
    pub latest: LatestTurn<'a>,
}

impl RoundStanding<'_> {
    /// The rail and the tab title of the round, by the rules of [`RoundOverview::of`], with no
    /// decisions and no earlier questions.
    pub fn overview(&self) -> RoundOverview {
        let position = Position::new(self.activity, self.latest, self.questions);
        RoundOverview {
            rail: RailStep::rail(self.design, self.questions, &position),
            decisions: Vec::new(),
            earlier: Vec::new(),
            title: TabTitle::of(&position),
        }
    }
}
