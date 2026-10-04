//! The Explore page: a browser page that shows an Explore round.
//!
//! The page is plain HTML rendered from minijinja templates, served by axum on its own
//! listener. The session that owns a round publishes its stage through a [`RoundPublisher`];
//! the page renders the latest one. The reviewer's actions (start a round, answer a question,
//! answer the conclusion's quiz, implement it, and every recovery the pane offers) are form
//! posts, which the page hands to the round's owner as [`PageCommand`]s through a
//! [`CommandSender`]; each post then redirects to the page. In every stage, a small script polls
//! the page's status and loads the page again once the round has changed. The page is tested with the
//! e2e tests in `tests/explore-page`, against the standalone server
//! (`review-explore-page-server`).
//!
//! Every request must name one of the page's own [`Hosts`], against DNS rebinding. The address
//! the reviewer opens carries a [`Token`], which the page trades for a cookie; every read of a
//! round needs that cookie.

mod access;
mod blind;
mod citation;
mod command;
mod diagram;
mod files;
mod form;
mod notice;
mod page;
mod round;
mod status;

pub use access::{Hosts, Token};
pub use command::{
    CommandRefusal, CommandReply, CommandSender, PageAnswer, PageCommand, PageImplement,
    PageQuizResponse, PageReply, Recovery,
};
pub use files::PageFiles;
pub use page::{ExplorePage, PageEvent};
pub use round::{
    ImplementationState, Interruption, LatestAnswer, PageImplementation, PageQuiz, PageRound,
    PublishedRound, QuestionMarks, ReviewName, RoundFeed, RoundPublisher, RoundStage, Rounds,
    TurnResponse,
};
