//! The settings of the Explore page, saved with the reviewer's other settings and changed in
//! the pane: what Start and Start with Challenger do in the pane, and whether, and where, the
//! reviewer serves the page to the network.

use serde::{Deserialize, Serialize};

/// The first port tried when the settings name none.
const DEFAULT_FIRST_PORT: u16 = 8790;

/// The Explore page's settings. A value missing from the saved settings, such as all of them in
/// settings saved before they existed, takes its default.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct ExplorePageSettings {
    pub pane_starts: PaneStarts,
    pub network: NetworkAccess,
}

impl ExplorePageSettings {
    /// Changes the one setting `setting` names to its value, keeping the others.
    pub fn set(&mut self, setting: ExplorePageSetting) {
        match setting {
            ExplorePageSetting::PaneStarts(starts) => self.pane_starts = starts,
            ExplorePageSetting::NetworkEnabled(enabled) => self.network.enabled = enabled,
            ExplorePageSetting::Interface(interface) => self.network.interface = interface,
            ExplorePageSetting::FirstPort(port) => self.network.first_port = port,
        }
    }
}

/// One setting of the Explore page with its new value. The pane saves a change as one setting,
/// so that it leaves the others as they are saved, by this reviewer or another one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExplorePageSetting {
    PaneStarts(PaneStarts),
    NetworkEnabled(bool),
    Interface(Option<String>),
    FirstPort(u16),
}

impl ExplorePageSetting {
    /// The interface `name`, as the reviewer typed it; an empty name stands for the interface
    /// of the route to the internet.
    pub fn interface(name: &str) -> Self {
        Self::Interface(interface_name(name))
    }

    /// The first port `text`, as the reviewer typed it, or why it is not a port from 1 to
    /// 65535.
    pub fn first_port(text: &str) -> Result<Self, String> {
        let text = text.trim();
        match text.parse::<u16>() {
            Ok(port) if port > 0 => Ok(Self::FirstPort(port)),
            _ => Err(format!(
                "The first port must be a number from 1 to 65535, not {text:?}"
            )),
        }
    }
}

/// What Start and Start with Challenger do in the pane, besides starting the round.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneStarts {
    /// They open the round's Explore page in the browser, and the round runs on the page.
    #[default]
    OnPage,
    /// They start the round in the pane, as Start in the pane does.
    InPane,
}

impl PaneStarts {
    /// Changes to the other choice.
    pub fn toggle(&mut self) {
        *self = match self {
            Self::OnPage => Self::InPane,
            Self::InPane => Self::OnPage,
        };
    }
}

/// Whether, and where, the reviewer serves its Explore page to the network. Turning it off keeps
/// the interface and the port for when it is turned on again.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct NetworkAccess {
    /// The page is served to the network; otherwise it stays on this machine.
    pub enabled: bool,
    /// The interface whose address the page listens on, such as `wlan0`; `None` for the
    /// interface of the route to the internet.
    pub interface: Option<String>,
    /// The first port tried; when another reviewer holds it, the page tries the next ones. 0
    /// takes any free port, which only the tests use.
    pub first_port: u16,
}

impl Default for NetworkAccess {
    fn default() -> Self {
        Self {
            enabled: true,
            interface: None,
            first_port: DEFAULT_FIRST_PORT,
        }
    }
}

impl NetworkAccess {
    /// Listens on the interface `name`, as the reviewer typed it; an empty name stands for the
    /// interface of the route to the internet.
    pub fn set_interface(&mut self, name: &str) {
        self.interface = interface_name(name);
    }
}

/// The interface `name` as the reviewer typed it; `None`, for the interface of the route to the
/// internet, when it is empty.
fn interface_name(name: &str) -> Option<String> {
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

#[cfg(test)]
#[path = "lib.tests.rs"]
mod tests;
