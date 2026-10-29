//! Opening the Explore page from the pane, as the pane and the page host both see it: the screen
//! the page opens on, and why the page did not open.

use std::fmt;

/// The screen the Explore page opens on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PageScreen {
    /// The round the page follows, or its start cover while no round runs.
    Current,
    /// The ⋯ menu's list of past rounds, over the round the page follows.
    PastRounds,
}

impl PageScreen {
    /// The fragment that the page's address ends with to open on this screen: the client reads
    /// it in `route.js`.
    pub fn fragment(self) -> &'static str {
        match self {
            Self::Current => "",
            Self::PastRounds => "#past-rounds",
        }
    }
}

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
