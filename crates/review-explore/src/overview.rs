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
pub use quiz::QuizStage;
pub use rail::{RailStep, Step, StepState};
pub use record::{AgentRecord, EarlierQuestion};
pub use title::TabTitle;

use serde::Serialize;

use crate::ExploreRound;
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
            rail: RailStep::rail(exploration, &steps, &position),
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
