//! Opening the Explore page from the pane, as the pane and the page host both see it: whether
//! Start and Start with Challenger open the page, and why the page did not open.

use std::fmt;

/// What Start and Start with Challenger do in the pane, besides starting the round:
/// `HERDR_REVIEWER_EXPLORE_BROWSER`, `on` by default.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PaneStarts {
    /// They open the round's Explore page in the browser, and the round runs on the page.
    #[default]
    OnPage,
    /// They start the round in the pane, as Start in the pane does.
    InPane,
}

impl PaneStarts {
    /// The variable that holds the setting.
    pub const VARIABLE: &str = "HERDR_REVIEWER_EXPLORE_BROWSER";

    /// The setting `value` of [`Self::VARIABLE`], when it is set.
    pub fn from_setting(value: Option<&str>) -> Result<Self, String> {
        match value {
            None | Some("on") => Ok(Self::OnPage),
            Some("off") => Ok(Self::InPane),
            Some(other) => Err(format!(
                "{} must be on or off, not {other:?}",
                Self::VARIABLE
            )),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_in_the_pane_open_the_page_unless_the_setting_is_off() {
        assert_eq!(PaneStarts::from_setting(None), Ok(PaneStarts::OnPage));
        assert_eq!(PaneStarts::from_setting(Some("on")), Ok(PaneStarts::OnPage));
        assert_eq!(
            PaneStarts::from_setting(Some("off")),
            Ok(PaneStarts::InPane)
        );
        assert!(PaneStarts::from_setting(Some("yes")).is_err());
    }
}
