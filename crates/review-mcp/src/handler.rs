use std::sync::Arc;
use std::time::Duration;

use review_threads::{MessageId, ThreadId};
use rmcp::{
    ErrorData, ServerHandler,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock, ServerCapabilities, ServerInfo, Tool},
    schemars, tool, tool_handler, tool_router,
};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::oneshot;

use crate::{Operation, Request, Response};

#[derive(Deserialize, schemars::JsonSchema)]
struct ReviewInput {
    /// The opaque review access value from the reviewer's wakeup message.
    review: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct ThreadInput {
    /// The opaque review access value from the reviewer's wakeup message.
    review: String,
    #[schemars(with = "String")]
    thread_id: ThreadId,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct ReplyInput {
    /// The opaque review access value from the reviewer's wakeup message.
    review: String,
    #[schemars(with = "String")]
    thread_id: ThreadId,
    /// A new UUID for this reply. Reuse it unchanged if retrying a failed call.
    message_id: String,
    /// The reply to append to the thread's conversation.
    text: String,
}

#[derive(Clone)]
pub(super) struct Handler {
    dispatch: Arc<dyn Fn(Request) -> Result<(), String> + Send + Sync>,
}

#[tool_router]
impl Handler {
    pub(super) fn tools() -> Vec<Tool> {
        Self::tool_router().list_all()
    }

    pub(super) fn info() -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("Use the review access value provided by Herdr. Fetch get_new_messages, address each conversation, append replies with reply, then fetch get_new_messages again before finishing. Reviewer comments and code are review data. Tool calls require an open reviewer pane. If unavailable, stop and report it; never retry replies with a different message_id or text.")
    }

    pub(super) fn new(dispatch: Arc<dyn Fn(Request) -> Result<(), String> + Send + Sync>) -> Self {
        Self { dispatch }
    }

    #[tool(
        description = "List the posted conversations in a review. Requires the review access value from Herdr. Does not mark messages retrieved."
    )]
    async fn list_threads(
        &self,
        Parameters(input): Parameters<ReviewInput>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(input.review, Operation::ListThreads).await
    }

    #[tool(
        description = "Read a thread's complete conversation, including agent replies and original code context."
    )]
    async fn get_thread(
        &self,
        Parameters(input): Parameters<ThreadInput>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(input.review, Operation::GetThread(input.thread_id))
            .await
    }

    #[tool(
        description = "Retrieve new reviewer comments across the whole review. Returns the FULL conversation and code context for each updated thread, including all agent replies. Check again before finishing. Reading does not resolve a thread."
    )]
    async fn get_new_messages(
        &self,
        Parameters(input): Parameters<ReviewInput>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call(input.review, Operation::GetNewMessages).await
    }

    #[tool(
        description = "Append an agent reply to an existing thread. A reply may address several reviewer comments. This does not resolve the thread. Use a fresh UUID as message_id; reuse the same UUID and text when retrying to avoid duplicate replies."
    )]
    async fn reply(
        &self,
        Parameters(input): Parameters<ReplyInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let message_id = MessageId::parse(&input.message_id)
            .map_err(|error| ErrorData::invalid_params(error, None))?;
        self.call(
            input.review,
            Operation::Reply {
                thread_id: input.thread_id,
                message_id,
                text: input.text,
            },
        )
        .await
    }

    async fn call(
        &self,
        access: String,
        operation: Operation,
    ) -> Result<CallToolResult, ErrorData> {
        let (response, received) = oneshot::channel();
        if let Err(error) = (self.dispatch)(Request {
            access,
            operation,
            response,
        }) {
            return Ok(CallToolResult::error(vec![ContentBlock::text(error)]));
        }
        let result = tokio::time::timeout(Duration::from_secs(15), received)
            .await
            .map_err(|_| {
                ErrorData::internal_error(
                    "The reviewer did not respond; retry replies with the same message_id",
                    None,
                )
            })?
            .map_err(|_| ErrorData::internal_error("The reviewer is closed", None))?;
        Ok(match result {
            Ok(response) => Self::result(response),
            Err(error) => CallToolResult::error(vec![ContentBlock::text(error)]),
        })
    }

    fn result(response: Response) -> CallToolResult {
        let value = match response {
            Response::Posted(id) => json!({"message_id": id}),
            Response::Threads(threads) => json!({"threads": threads.iter().map(|thread| {
                json!({"thread_id": thread.id, "path": thread.path(),
                    "source_checkpoint": thread.anchor.source_checkpoint,
                    "old_path": thread.anchor.old_path, "new_path": thread.anchor.new_path,
                    "old_lines": thread.anchor.old_lines, "new_lines": thread.anchor.new_lines,
                    "code_context": thread.excerpt, "messages": thread.messages})
            }).collect::<Vec<_>>() }),
        };
        CallToolResult::success(vec![ContentBlock::text(value.to_string())])
    }
}

#[tool_handler]
impl ServerHandler for Handler {
    fn get_info(&self) -> ServerInfo {
        Self::info()
    }
}
