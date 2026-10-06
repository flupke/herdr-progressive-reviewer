//! The messages of the page's socket, in the shape of JSON-RPC 2.0: the page's requests carry an
//! `id`, a `method` and its `params`, and each gets one reply with that `id` and a `result` or an
//! `error`; the tool's own messages are notifications, a `method` with no `id`. The `id` only
//! matches a reply to its request: what makes a repeated action change nothing is the identity
//! of what it acts on, which its params carry (the question and its version, the turn's request,
//! the conclusion's request, the delivery, the round).

use review_threads::AskedUnder;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::status::StatusCard;
use crate::view::PageView;

/// A message the tool sends the page on its own.
#[derive(Debug, Serialize, TS)]
#[serde(tag = "method", content = "params", rename_all = "kebab-case")]
pub(crate) enum Notification {
    /// The round as the page shows it now, whole.
    State(Box<StateParams>),
    /// The tool is still there: a page that hears nothing for a while opens a new socket.
    Ping,
}

/// The whole view, numbered: `seq`, compared as a pair, goes up with each change the page shows,
/// and starts again with a new `epoch` when the tool restarts.
#[derive(Debug, Serialize, TS)]
pub(crate) struct StateParams {
    pub(crate) epoch: String,
    pub(crate) seq: Seq,
    pub(crate) view: PageView,
}

/// The number of a view: the revision of the round's published stage, then how many first
/// picks this page kept for the round, then the revision of the review threads, which hold the
/// round's conversation: the last two change the view without changing the first. Each part
/// only goes up.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, TS)]
pub(crate) struct Seq {
    pub(crate) revision: u64,
    pub(crate) picks: u64,
    pub(crate) threads: u64,
}

/// A request of the page: the reviewer's action, or a check that the tool is there.
#[derive(Debug, Deserialize, TS)]
pub(crate) struct Request {
    /// Matches the reply to the request, on this socket only.
    pub(crate) id: u64,
    #[serde(flatten)]
    pub(crate) call: Call,
}

/// What the page asks, with the identities of what it acts on: a `method` and its `params`,
/// on the round, on its conclusion, or in its conversation with the agent.
#[derive(Debug, Deserialize, TS)]
#[serde(untagged)]
pub(crate) enum Call {
    Round(RoundCall),
    Conclusion(ConclusionCall),
    Conversation(ConversationCall),
}

/// What the page asks of the round.
#[derive(Debug, Deserialize, TS)]
#[serde(tag = "method", content = "params", rename_all = "kebab-case")]
pub(crate) enum RoundCall {
    /// Answer the question the page showed.
    Answer(AnswerParams),
    /// The reviewer's first pick of a blind question, before its recommendation shows.
    Pick(PickParams),
    /// Start a round, from the start screen the page showed.
    Start(StartParams),
    /// Stop waiting for the start or the agent's turn the page showed.
    Stop(StopParams),
    /// Send again the agent's turn the page showed as interrupted.
    Retry(RetryParams),
    /// Take back the reviewer's latest answer.
    CancelAnswer(CancelAnswerParams),
    /// Close the round the page showed.
    Reset(ResetParams),
    /// Mermaid could not draw a diagram of the question the page showed.
    DiagramFailed(DiagramParams),
    /// Whether the tool is there; changes nothing.
    Ping,
}

/// What the page asks of the round's conclusion.
#[derive(Debug, Deserialize, TS)]
#[serde(tag = "method", content = "params", rename_all = "kebab-case")]
pub(crate) enum ConclusionCall {
    /// Implement the conclusion the page showed.
    Implement(ImplementParams),
    /// Pick an option of an item of the conclusion's quiz.
    Quiz(QuizParams),
    /// Skip the rest of the conclusion's quiz.
    QuizSkip(QuizSkipParams),
    /// Cancel the implementation request the page showed as being sent.
    CancelImplementation(CancelImplementationParams),
    /// Send again, as it is, the implementation request the page showed.
    ResendImplementation(ResendImplementationParams),
}

#[derive(Debug, Deserialize, TS)]
pub(crate) struct AnswerParams {
    /// The identity of the round that showed the question, when it has one.
    pub(crate) round: Option<String>,
    pub(crate) question: String,
    pub(crate) version: u32,
    /// The picked choice's ID; `None` when the reviewer picked none.
    pub(crate) choice: Option<String>,
    pub(crate) comment: String,
    /// The question's number on the rail, as the page showed it, which a refusal names.
    #[serde(default)]
    pub(crate) number: Option<usize>,
}

#[derive(Debug, Deserialize, TS)]
pub(crate) struct PickParams {
    pub(crate) round: Option<String>,
    pub(crate) question: String,
    pub(crate) version: u32,
    pub(crate) choice: String,
    /// The question's number on the rail, as the page showed it, which a refusal names.
    #[serde(default)]
    pub(crate) number: Option<usize>,
}

#[derive(Debug, Deserialize, TS)]
pub(crate) struct StartParams {
    /// Whether the Challenger reviews the change beside the agent.
    pub(crate) challenger: bool,
    /// The identity of the start the page offered.
    pub(crate) start: String,
}

#[derive(Debug, Deserialize, TS)]
pub(crate) struct StopParams {
    /// The start the page showed under way, by its identity.
    #[serde(default)]
    pub(crate) start: Option<String>,
    /// The agent's turn the page showed the agent working on.
    #[serde(default)]
    pub(crate) request: Option<String>,
}

#[derive(Debug, Deserialize, TS)]
pub(crate) struct RetryParams {
    pub(crate) request: String,
    /// The turn's latest attempt, as the page showed it.
    pub(crate) attempt: String,
}

#[derive(Debug, Deserialize, TS)]
pub(crate) struct CancelAnswerParams {
    pub(crate) answer: String,
}

#[derive(Debug, Deserialize, TS)]
pub(crate) struct ResetParams {
    pub(crate) round: String,
}

#[derive(Debug, Deserialize, TS)]
pub(crate) struct ImplementParams {
    pub(crate) conclusion: String,
    /// The delivery of the conclusion's request that the page showed as not sent; `None` when
    /// it showed none.
    pub(crate) replaces: Option<String>,
    pub(crate) text: String,
}

#[derive(Debug, Deserialize, TS)]
pub(crate) struct QuizParams {
    pub(crate) conclusion: String,
    /// The item, from 0.
    pub(crate) item: usize,
    /// The option picked, from 0.
    pub(crate) answer: usize,
}

#[derive(Debug, Deserialize, TS)]
pub(crate) struct QuizSkipParams {
    pub(crate) conclusion: String,
}

/// What the page asks in the round's conversation with the agent, a review thread: the
/// reviewer's messages do not answer the round's questions.
#[derive(Debug, Deserialize, TS)]
#[serde(tag = "method", content = "params", rename_all = "kebab-case")]
pub(crate) enum ConversationCall {
    /// Post the reviewer's message in the round's conversation, which wakes the agent.
    SendMessage(MessageParams),
    /// The chat showed the agent's replies: mark them read.
    ReadMessages(ReadParams),
    /// Wake the agent again for the reviewer's messages that did not reach it.
    RetryMessages(RetryMessagesParams),
}

#[derive(Debug, Deserialize, TS)]
pub(crate) struct MessageParams {
    /// The round the page showed, whose conversation the message joins.
    pub(crate) round: String,
    /// The message's identity, a UUID the page chose, so that sending it again posts it once.
    pub(crate) id: String,
    pub(crate) text: String,
    /// Where in the round the page showed the chat: the question and its version, the design,
    /// or the conclusion; `None` elsewhere.
    pub(crate) asked_under: Option<AskedUnder>,
    /// The passage of the round the reviewer quoted.
    pub(crate) quote: Option<String>,
}

#[derive(Debug, Deserialize, TS)]
pub(crate) struct ReadParams {
    pub(crate) round: String,
    /// The position the view said to mark read through.
    pub(crate) through: u64,
}

#[derive(Debug, Deserialize, TS)]
pub(crate) struct RetryMessagesParams {
    pub(crate) round: String,
}

#[derive(Debug, Deserialize, TS)]
pub(crate) struct CancelImplementationParams {
    pub(crate) delivery: String,
}

#[derive(Debug, Deserialize, TS)]
pub(crate) struct ResendImplementationParams {
    pub(crate) conclusion: String,
    pub(crate) delivery: String,
    /// The request's latest attempt, as the page showed it.
    pub(crate) attempt: String,
}

/// A diagram of the question the page showed that Mermaid could not parse.
#[derive(Debug, Deserialize, TS)]
pub(crate) struct DiagramParams {
    pub(crate) question: String,
    pub(crate) version: u32,
    /// The diagram's Mermaid source.
    pub(crate) source: String,
    /// Mermaid's message.
    pub(crate) message: String,
}

/// The reply to one request.
#[derive(Debug, Serialize, TS)]
#[serde(untagged)]
pub(crate) enum Reply {
    Result { id: u64, result: Outcome },
    Error { id: u64, error: RpcError },
}

/// What became of an action the tool carried out.
#[derive(Debug, Default, Serialize, TS)]
pub(crate) struct Outcome {
    /// Whether this request changed the round; false for a repeat of an action applied
    /// already, which changes nothing.
    pub(crate) applied: bool,
    /// After a Reset on the network: the token of the start screen, which the page opens with
    /// from now on, since the reset round's token opens nothing any more.
    pub(crate) reopen: Option<String>,
}

impl Outcome {
    /// Whether the request changed the round.
    pub(crate) fn applied(applied: bool) -> Self {
        Self {
            applied,
            reopen: None,
        }
    }
}

/// Why the tool did not carry out a request.
#[derive(Debug, Serialize, TS)]
pub(crate) struct RpcError {
    pub(crate) code: i32,
    pub(crate) message: String,
    /// The notice the page shows, worded for the action, as a status card; `None` for a request
    /// the page could not have sent, and for one sent after the page's token stopped opening the
    /// round, which the socket closes once the replies it waits for are sent.
    pub(crate) data: Option<StatusCard>,
}

impl RpcError {
    /// A request the page could not have sent: not JSON, or not one of the methods.
    pub(crate) const INVALID_REQUEST: i32 = -32600;
    /// The round moved on since the page showed what the action acts on.
    pub(crate) const STALE: i32 = 409;
    /// The round's owner could not carry out the action.
    pub(crate) const FAILED: i32 = 500;
    /// The round's owner did not reply: the action may have gone through.
    pub(crate) const NO_REPLY: i32 = 504;
}
