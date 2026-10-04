//! Opening the Explore page from the pane, as the pane and the page host both see it: why the
//! page did not open.

use std::fmt;

/// Why the browser did not open the Explore page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PageNotOpened {
    /// The address of the page, with its token, to open by hand; `None` when no reviewer of the
    /// workspace serves a page.
    pub url: Option<String>,
    pub reason: String,
}

impl fmt::Display for PageNotOpened {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Cannot open the Explore page: {}", self.reason)
    }
}

impl std::error::Error for PageNotOpened {}
