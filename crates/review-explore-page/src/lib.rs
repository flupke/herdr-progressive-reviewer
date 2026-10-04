//! The Explore page: a browser page that shows an Explore round.
//!
//! The page is a small JavaScript client (`assets/client`), served by axum on its own listener
//! with the page's styles. The session that owns a round publishes its stage through a
//! [`RoundPublisher`]; each open page holds one socket at `/ws`, which sends it the latest stage
//! as typed data, whole, when it opens and at each change, and carries the reviewer's actions
//! (start a round, answer a question, answer the conclusion's quiz, implement it, and every
//! recovery the pane offers) back, which the page hands to the round's owner as
//! [`PageCommand`]s through a [`CommandSender`]. The page is tested with the e2e tests in
//! `tests/explore-page`, against the standalone server (`review-explore-page-server`).
//!
//! Every request must name one of the page's own [`Hosts`], against DNS rebinding. The address
//! the reviewer opens carries a [`Token`], which the page trades for a cookie; every read of a
//! round needs that cookie, the socket's upgrade included.

mod access;
mod actions;
mod blind;
mod command;
mod conversation;
mod diagram;
mod files;
mod notice;
mod page;
mod round;
mod rpc;
mod socket;
mod status;
#[cfg(test)]
mod typescript;
mod view;

pub use access::{Hosts, Token};
pub use command::{
    CommandRefusal, CommandReply, CommandSender, PageAnswer, PageCommand, PageImplement,
    PageQuizResponse, Recovery, Waiting,
};
pub use conversation::{PageConversation, ThreadSender, ThreadsFeed, ThreadsPublisher};
pub use files::PageFiles;
pub use page::{ExplorePage, PageEvent};
pub use round::{
    Answered, AnsweredQuestion, ImplementationState, Interruption, LatestAnswer,
    PageImplementation, PageQuiz, PageRound, PublishedRound, QuestionMarks, ReviewName, RoundFeed,
    RoundPublisher, RoundStage, Rounds, SentAnswer, TurnResponse,
};
