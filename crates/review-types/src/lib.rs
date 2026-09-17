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
    pub vim: bool,
    pub normal: bool,
}
