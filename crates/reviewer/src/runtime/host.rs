//! The agent host the review pane runs in: Herdr, or herdr-core under herdr-app.

use std::ffi::OsString;
use std::sync::Arc;

use hc_client::HcClient;
use herdr_client::client::HerdrClient;
use herdr_client::protocol::AgentHost;

/// Which host runs the review pane.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HostKind {
    Herdr,
    /// herdr-core (`hc`), the core of herdr-app.
    HerdrCore,
}

/// Which host runs the review pane, by the variable each gives its terminals, which `var`
/// reads. Herdr's wins: a Herdr started from a terminal of herdr-app may pass that terminal's
/// herdr-core variables on to its panes, while its agents live in Herdr.
fn host_kind(var: impl Fn(&str) -> Option<OsString>) -> Option<HostKind> {
    let set = |name: &str| var(name).is_some_and(|value| !value.is_empty());
    if set("HERDR_SOCKET_PATH") {
        Some(HostKind::Herdr)
    } else if set("HC_TERMINAL_SOCKET") {
        Some(HostKind::HerdrCore)
    } else {
        None
    }
}

/// The host of the review pane, from the environment Herdr or herdr-core gives it.
pub(super) fn from_env() -> eyre::Result<Arc<dyn AgentHost>> {
    Ok(match host_kind(|name| std::env::var_os(name)) {
        Some(HostKind::Herdr) => Arc::new(HerdrClient::from_env()?),
        Some(HostKind::HerdrCore) => Arc::new(HcClient::from_env()?),
        None => eyre::bail!(
            "neither HERDR_SOCKET_PATH nor HC_TERMINAL_SOCKET is set: the reviewer runs in a \
             pane of Herdr or a terminal of herdr-app"
        ),
    })
}

#[cfg(test)]
#[path = "host.tests.rs"]
mod tests;
