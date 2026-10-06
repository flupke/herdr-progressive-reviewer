//! The tunnel that shares the running Explore round beyond this machine's network, as the pane
//! and the page host both see it.

/// Where the tunnel of the running round stands. The reviewer turns it on for the round that
/// runs; it goes off with that round, with the reviewer, or when the reviewer turns it off.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum TunnelState {
    /// No tunnel runs.
    #[default]
    Off,
    /// The tunnel is starting: no public address yet.
    Opening,
    /// The tunnel runs: the public address of the round's page, with the round's token.
    Open { url: String },
    /// The tunnel did not open, or went down, for this reason, in one line. No tunnel runs.
    Failed(String),
}

impl TunnelState {
    /// Whether a tunnel runs or is starting: the reviewer's control turns it off.
    pub fn is_on(&self) -> bool {
        matches!(self, Self::Opening | Self::Open { .. })
    }
}
