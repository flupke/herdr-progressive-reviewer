//! The reviewer's settings for the Explore rounds it starts, which shape the agent's prompts:
//! the writing style of the texts the reviewer reads. They are saved with the reviewer's other
//! settings and changed in the pane. A round keeps the values it started with.

use serde::{Deserialize, Serialize};

/// The settings for the next Explore round. A value missing from the saved settings, such as
/// all of them in settings saved before they existed, takes its default.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct ExploreRoundSettings {
    pub writing: WritingStyle,
}

/// How the Explore agent writes every text the reviewer reads: the design, the questions with
/// their context, choices, door and blast radius, its replies, the conclusion and the quiz.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WritingStyle {
    /// The agent's own style, as the prompts asked before writing styles existed.
    Plain,
    /// ASD-STE100 Simplified Technical English: short sentences, one topic each, in the
    /// active voice, with technical names kept as they are in the code.
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

#[cfg(test)]
#[path = "lib.tests.rs"]
mod tests;
