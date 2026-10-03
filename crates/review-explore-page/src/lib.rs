//! The Explore page: a browser page that shows an Explore round.
//!
//! The page is plain HTML rendered from minijinja templates, served by axum on its own
//! listener. The session that owns a round publishes its stage through a [`RoundPublisher`];
//! the page renders the latest one. While the agent works, a small script polls the page's
//! status and loads the page again once the round has changed. The page is tested with the
//! e2e tests in `tests/explore-page`, against the standalone server
//! (`review-explore-page-server`).
//!
//! Every request must name one of the page's own [`Hosts`], against DNS rebinding. The address
//! the reviewer opens carries a [`Token`], which the page trades for a cookie; every read of a
//! round needs that cookie.

mod access;
mod citation;
mod files;
mod page;
mod round;

pub use access::{Hosts, Token};
pub use files::PageFiles;
pub use page::{ExplorePage, PageEvent};
pub use round::{RoundFeed, RoundPublisher, RoundStage, Rounds};
