//! Run-ahead: while a question of an Explore round waits for the reviewer, forks of the pane
//! agent's session take, in the background, the turn that would follow each choice. Each fork
//! gets the ordinary prompt of that answer and its own access value; the tool keeps what it
//! submits for its choice and shows it to nobody.
//!
//! This crate holds what the tool saves of its forks beside each round ([`RoundForks`]) and the
//! interface to the agent whose session it forks ([`ForkHost`]), which hides what is specific
//! to that agent. The Explore session decides when forks start and stop.

mod host;
mod record;

pub use host::{ForkEnd, ForkHost, ForkPoint, ForkStart, ForkTrace, PaneWatch, StatusReport};
pub use record::{Discard, DiscardReason, ForkRecord, RoundForks, TokenUsage};
