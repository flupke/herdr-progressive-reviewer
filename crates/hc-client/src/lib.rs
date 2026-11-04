//! The reviewer's agent host under herdr-app: a client of herdr-core (`hc`), the core that runs
//! herdr-app's terminals and agents, behind herdr-client's [`AgentPort`] and [`AgentHost`].
//!
//! An agent is a terminal whose program reports its status with OSC 7501, in the tab of
//! herdr-app its `herdr.tab` label names. herdr-core keeps no focus and no agent session, so
//! the agents it gives have none, and an agent's arrival or departure is the appearance or the
//! end of its status.
//!
//! [`AgentPort`]: herdr_client::protocol::AgentPort
//! [`AgentHost`]: herdr_client::protocol::AgentHost

mod client;
mod events;
mod terminal;

pub use client::HcClient;
