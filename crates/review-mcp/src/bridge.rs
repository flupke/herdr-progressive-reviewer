use std::time::Duration;

use rmcp::{
    ErrorData, RoleServer, ServerHandler, ServiceExt,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ClientInfo, ContentBlock,
        ListToolsResult, PaginatedRequestParams, ServerInfo,
    },
    service::RequestContext,
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};

use crate::{Endpoint, handler::Handler};

/// Keep tool discovery independent of the lifetime of the reviewer pane.
pub fn serve_stdio(endpoint: Endpoint) -> Result<(), String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?
        .block_on(async {
            Bridge { endpoint }
                .serve(rmcp::transport::stdio())
                .await
                .map_err(|error| error.to_string())?
                .waiting()
                .await
                .map_err(|error| error.to_string())?;
            Ok(())
        })
}

struct Bridge {
    endpoint: Endpoint,
}

impl Bridge {
    async fn forward(&self, request: CallToolRequestParams) -> Result<CallToolResult, String> {
        let client = ClientInfo::default()
            .serve(StreamableHttpClientTransport::from_config(
                StreamableHttpClientTransportConfig::with_uri(self.endpoint.url())
                    .reinit_on_expired_session(false),
            ))
            .await
            .map_err(|_| {
                "The reviewer is closed or unavailable; open it and retry the same call".to_owned()
            })?;
        // Never replay a tool call automatically: retrieval and replies mutate review state.
        let result = client
            .call_tool(request)
            .await
            .map_err(|error| error.to_string());
        let _ = client.cancel().await;
        result
    }
}

impl ServerHandler for Bridge {
    fn get_info(&self) -> ServerInfo {
        Handler::info()
    }

    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(Handler::tools()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let result = tokio::time::timeout(Duration::from_secs(20), self.forward(request)).await;
        let result = result.unwrap_or_else(|_| {
            Err(
                "The reviewer did not respond; retry replies with the same message_id and text"
                    .into(),
            )
        });
        Ok(result
            .unwrap_or_else(|error| CallToolResult::error(vec![ContentBlock::text(error)]))
            .into())
    }
}

#[cfg(test)]
#[path = "bridge.tests.rs"]
mod tests;
