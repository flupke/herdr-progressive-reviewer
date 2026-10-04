//! What run-ahead did with each answer to a question it watched: the pane's agent continued as
//! the fork for the answer's choice, or took the turn itself, and why.

use review_turn_path::TurnPath;
use serde::{Deserialize, Serialize};

/// One answer of the reviewer to a question run-ahead watched, and the path its turn took.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct AnswerRecord {
    /// The question answered, by ID and version.
    pub question: String,
    pub version: u32,
    /// The reviewer's answer, by ID.
    pub answer: String,
    /// The turn the answer started, by request.
    pub request: String,
    /// When the reviewer answered, in milliseconds since the epoch.
    pub at_ms: u64,
    pub path: TurnPath,
}
