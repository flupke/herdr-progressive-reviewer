//! Why a quick tunnel gave no address, or stopped.

use std::fmt;
use std::time::Duration;

use agent_fork::Exit;

/// Where to get `cloudflared`.
const INSTALL: &str =
    "https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/";

/// Why a quick tunnel gave no public address, or stopped after it gave one. Each reads as one
/// line, for the pane.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TunnelFailure {
    /// The program is not installed: not on the `PATH`, or not executable where it was named.
    Missing { program: String },
    /// The program could not start.
    NotStarted(String),
    /// The process ended before it printed an address.
    Exited(Exit),
    /// The process printed no address within this wait, and was stopped.
    NoAddress(Duration),
    /// The process ended after it gave the address: the tunnel is down.
    Ended(Exit),
}

impl fmt::Display for TunnelFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing { program } => write!(
                formatter,
                "{program} is not installed: install cloudflared from your package manager or \
                 {INSTALL}"
            ),
            Self::NotStarted(error) => write!(formatter, "cloudflared did not start: {error}"),
            Self::Exited(exit) => write!(
                formatter,
                "cloudflared stopped before it gave an address: {}",
                last_words(exit)
            ),
            Self::NoAddress(wait) => write!(
                formatter,
                "cloudflared gave no address within {} seconds",
                wait.as_secs()
            ),
            Self::Ended(exit) => write!(formatter, "cloudflared stopped: {}", last_words(exit)),
        }
    }
}

impl std::error::Error for TunnelFailure {}

/// The last line `cloudflared` wrote on its standard error, which says why it stopped, or else
/// how it ended.
fn last_words(exit: &Exit) -> &str {
    exit.stderr
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .unwrap_or(&exit.status)
}
