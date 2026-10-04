//! The reviewer's settings for its Explore rounds: the writing style of the texts the reviewer
//! reads, which shapes the agent's prompts, and run-ahead, which prepares the agent's next turn
//! while the reviewer thinks. They are saved with the reviewer's other settings and changed in
//! the pane. A round keeps the writing style it started with; run-ahead applies from the next
//! question the agent posts, since it changes no prompt of the round.

use serde::{Deserialize, Serialize};

/// The settings for the Explore rounds. A value missing from the saved settings, such as all
/// of them in settings saved before they existed, takes its default.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct ExploreRoundSettings {
    pub writing: WritingStyle,
    pub run_ahead: RunAhead,
}

/// How the Explore agent writes every text the reviewer reads: the design, the questions with
/// their context, choices, door and blast radius, its replies, the conclusion and the quiz.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WritingStyle {
    /// The agent's own style, as the prompts asked before writing styles existed.
    Plain,
    /// The writing rules of ASD-STE100 Simplified Technical English, without its controlled
    /// dictionary: short full sentences, one topic each, in the active voice, with technical
    /// names kept as they are in the code.
    #[default]
    SimplifiedTechnicalEnglish,
}

impl WritingStyle {
    /// Whether this is the plain style, which a saved round leaves out.
    pub fn is_plain(&self) -> bool {
        *self == Self::Plain
    }

    /// Changes to the other style.
    pub fn toggle(&mut self) {
        *self = match self {
            Self::Plain => Self::SimplifiedTechnicalEnglish,
            Self::SimplifiedTechnicalEnglish => Self::Plain,
        };
    }
}

/// Which choices of a question that waits run-ahead prepares the agent's next turn for: one
/// fork of the agent's session per choice takes the turn that would follow that answer, in the
/// background. Off by default until the feature is complete.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunAhead {
    /// No fork runs.
    #[default]
    Off,
    /// Only the choice the agent recommends, when it recommends one.
    Recommended,
    /// Every choice the question offers.
    Every,
}

impl RunAhead {
    /// Whether run-ahead may prepare a choice at all.
    pub fn is_on(self) -> bool {
        self != Self::Off
    }

    /// Whether run-ahead prepares a choice, which the agent recommends or not.
    pub fn prepares(self, recommended: bool) -> bool {
        match self {
            Self::Off => false,
            Self::Recommended => recommended,
            Self::Every => true,
        }
    }

    /// Changes to the next value: off, then the recommended choice, then every choice.
    pub fn cycle(&mut self) {
        *self = match self {
            Self::Off => Self::Recommended,
            Self::Recommended => Self::Every,
            Self::Every => Self::Off,
        };
    }
}

#[cfg(test)]
#[path = "lib.tests.rs"]
mod tests;
