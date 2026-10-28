//! Streamable HTTP tools for the conversation service owned by an open reviewer.

mod bridge;
mod endpoint;
mod handler;
mod instructions;
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
    Reply(Box<Post>),
    SubmitQuestion(Box<review_explore::InterviewUpdate>),
    SubmitConclusion(Box<review_explore::ConclusionSubmission>),
    ReviseQuestion(Box<review_explore::QuestionRevision>),
}

/// An in-process request from an MCP handler to the reviewer-owned service.
#[derive(Debug)]
pub struct Request {
    /// An opaque capability supplied in the reviewer's Herdr wakeup message.
    pub access: String,
    pub operation: Operation,
    /// The agent session the call came from, as the agent names it in the call's metadata:
    /// Codex's `_meta.threadId`. `None` for an agent that names none, such as Claude Code.
    pub agent_session: Option<String>,
    response: oneshot::Sender<Result<Response, String>>,
}

impl Operation {
    /// Explore operations belong to the Explore session, not to review threads.
    pub fn belongs_to_explore(&self) -> bool {
        matches!(
            self,
            Self::SubmitQuestion(_) | Self::SubmitConclusion(_) | Self::ReviseQuestion(_)
        )
    }
}

/// The owner's eventual answer to one [`Request`].
pub type PendingResponse = oneshot::Receiver<Result<Response, String>>;

impl Request {
    /// A request whose answer arrives on the returned receiver once the owner responds.
    pub fn new(access: String, operation: Operation) -> (Self, PendingResponse) {
        let (response, received) = oneshot::channel();
        (
            Self {
                access,
                operation,
                agent_session: None,
                response,
            },
            received,
        )
    }

    /// Finish a request after the authoritative owner has accepted its result.
    pub fn respond(self, result: Result<Response, String>) {
        let _ = self.response.send(result);
    }
}

/// Authoritative results returned by the conversation owner.
#[derive(Debug)]
pub enum Response {
    Threads(Vec<ReviewThread>),
    /// The threads that wait for the agent's answer, which the result gives with the
    /// instructions for answering them.
    NewMessages(Vec<ReviewThread>),
    Posted(MessageId),
    /// Whether an Explore submission was new rather than a retry, and the number the reviewer's
    /// screens give the question it posted; `None` for a conclusion.
    Explore {
        applied: bool,
        shown_as: Option<review_types::QuestionNumber>,
    },
}

#[cfg(test)]
mod tests;
