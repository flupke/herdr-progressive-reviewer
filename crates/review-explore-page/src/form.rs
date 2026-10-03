//! What the page reads from the reviewer's forms.

use serde::{Deserialize, Deserializer};

/// The text of a text area, as the reviewer wrote it. A browser posts a text area's line breaks
/// as CRLF, while the pane's editor keeps LF: the text keeps LF, so that the round saves the
/// same text whichever front end the reviewer wrote it in.
#[derive(Default)]
pub(crate) struct TextArea(String);

impl TextArea {
    pub(crate) fn into_string(self) -> String {
        self.0
    }
}

impl<'de> Deserialize<'de> for TextArea {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let posted = String::deserialize(deserializer)?;
        Ok(Self(posted.replace("\r\n", "\n")))
    }
}
