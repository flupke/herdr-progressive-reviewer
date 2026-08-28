//! Types shared by the progressive review crates.

use serde::{Deserialize, Serialize};

/// The stable identity of one logical review.
#[derive(Clone, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
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
