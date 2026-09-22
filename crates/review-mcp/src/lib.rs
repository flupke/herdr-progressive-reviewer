//! Streamable HTTP tools for the conversation service owned by an open reviewer.

mod bridge;
mod endpoint;
mod handler;
mod server;

use review_threads::{MessageId, Post, ReviewThread, ThreadId};
use tokio::sync::oneshot;

pub use bridge::serve_stdio;
pub use endpoint::Endpoint;
pub use server::Server;

/// An agent operation, validated and executed by the conversation's serial owner.
#[derive(Debug)]
pub enum Operation {
    ListThreads,
    GetThread(ThreadId),
    GetNewMessages,
    Reply(Post),
    SubmitQuestion(Box<review_explore::InterviewUpdate>),
    SubmitConclusion(Box<review_explore::ConclusionSubmission>),
}

/// An in-process request from an MCP handler to the reviewer-owned service.
#[derive(Debug)]
pub struct Request {
    /// An opaque capability supplied in the reviewer's Herdr wakeup message.
    pub access: String,
    pub operation: Operation,
    response: oneshot::Sender<Result<Response, String>>,
}

impl Request {
    /// Finish a request after the authoritative owner has accepted its result.
    pub fn respond(self, result: Result<Response, String>) {
        let _ = self.response.send(result);
    }
}

/// Authoritative results returned by the conversation owner.
#[derive(Debug)]
pub enum Response {
    Threads(Vec<ReviewThread>),
    Posted(MessageId),
    Explore {
        applied: bool,
        coverage: review_explore::CoverageFeedback,
    },
}

#[cfg(test)]
mod tests;
