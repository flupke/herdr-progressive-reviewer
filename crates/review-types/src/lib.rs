//! Types shared by the progressive review crates.

use serde::{Deserialize, Serialize};

/// The stable identity of one logical review.
#[derive(
    Clone, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize, schemars::JsonSchema,
)]
#[serde(transparent)]
pub struct ReviewUnit(String);

impl ReviewUnit {
    /// Get the identity text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Return whether the identity is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<String> for ReviewUnit {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for ReviewUnit {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

/// Portable editor content and reading position; no component, cache or undo machinery.
#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
pub struct TextEditorState {
    pub text: String,
    pub row: usize,
    pub column: usize,
    pub scroll: usize,
    /// Whether a Vim editor was in Normal mode.
    pub normal: bool,
}

/// Choose modal Vim commands or regular, always-inserting text input.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EditorKeymap {
    #[default]
    Vim,
    Regular,
}

/// Who marked changed lines reviewed.
#[derive(Clone, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MarkAuthor {
    /// The reviewer, marking in the review UI.
    #[default]
    Reviewer,
    /// Jev, marking lines too insignificant to need the reviewer's attention.
    Jev,
    /// An Explore agent, marking the lines one answer settled.
    Explore {
        /// The answer that applied the marks.
        answer: String,
    },
    /// An Explore agent, marking lines it read and found to hold no decision
    /// for the reviewer, on a turn that follows no answer.
    ExploreRead {
        /// The request of the turn that marked them.
        request: String,
    },
}
