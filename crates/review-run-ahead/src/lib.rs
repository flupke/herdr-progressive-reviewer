//! Run-ahead: while a question of an Explore round waits for the reviewer, forks of the pane
//! agent's session take, in the background, the turn that would follow each choice. Each fork
//! gets the ordinary prompt of that answer and its own access value; the tool keeps what it
//! submits for its choice and shows it to nobody.
//!
//! When the reviewer's answer is exactly the one a fork was told, the pane's agent continues
//! as that fork: it resumes the fork's session, and the fork's turn becomes the round's.
//!
//! This crate holds what the tool saves of its forks and of the answers beside each round
//! ([`RoundForks`]) and the interface to the agent whose session it forks ([`ForkHost`]),
//! which hides what is specific to that agent. The Explore session decides when forks start
//! and stop, and which answer continues as a fork.

mod answer;
mod host;
mod record;

pub use answer::AnswerRecord;
pub use host::{
    ForkEnd, ForkHost, ForkPoint, ForkStart, ForkTrace, PaneWatch, StatusReport, SwitchFailure,
    SwitchTo,
};
pub use record::{
    Continuation, Discard, DiscardReason, FAILURES_TO_HALT, ForkRecord, RoundForks, TokenUsage,
};
pub use review_turn_path::{PlainReason, TurnPath};
