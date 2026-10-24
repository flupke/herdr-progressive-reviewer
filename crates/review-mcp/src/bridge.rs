use std::time::Duration;

use rmcp::{
    ErrorData, RoleServer, ServerHandler, ServiceExt,
    model::{
        CacheScope, CallToolRequestParams, CallToolResponse, CallToolResult, ClientInfo,
        ContentBlock, ListToolsResult, PaginatedRequestParams, RequestMetaObject, ResultType,
        ServerInfo,
    },
    service::RequestContext,
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};

use crate::{Endpoint, handler::Handler};

/// Keep tool discovery independent of the lifetime of the reviewer pane.
pub fn serve_stdio(endpoint: Result<Endpoint, String>) -> Result<(), String> {
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
    endpoint: Result<Endpoint, String>,
}

impl Bridge {
    async fn forward(&self, request: CallToolRequestParams) -> Result<CallToolResult, String> {
        let endpoint = self.endpoint.as_ref().map_err(Clone::clone)?;
        let client = ClientInfo::default()
            .serve(StreamableHttpClientTransport::from_config(
                StreamableHttpClientTransportConfig::with_uri(endpoint.url())
                    .reinit_on_expired_session(false),
            ))
            .await
            .map_err(|_| {
                "The reviewer is closed or unavailable; open it and retry the same call".to_owned()
            })?;
        // Never replay tool calls automatically: a reply may already have been saved.
        let result = client
            .call_tool(request)
            .await
            .map_err(|error| error.to_string());
        let _ = client.cancel().await;
        // The reviewer answers over an earlier protocol revision, which omits the
        // discriminator that the agent's revision requires on every result.
        result.map(|mut result| {
            result.result_type = Some(ResultType::COMPLETE);
            result
        })
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
        // Protocol 2026-07-28 requires cache hints; clients reject a catalog without them.
        let mut catalog = ListToolsResult::with_all_items(Handler::tools());
        catalog.ttl_ms = Some(0);
        catalog.cache_scope = Some(CacheScope::Private);
        Ok(catalog)
    }

    async fn call_tool(
        &self,
        mut request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        request.meta = agent_meta(context.meta);
        let result = tokio::time::timeout(Duration::from_secs(20), self.forward(request)).await;
        let result = result.unwrap_or_else(|_| {
            Err("The reviewer did not respond; retry the identical tool arguments".into())
        });
        Ok(result
            .unwrap_or_else(|error| CallToolResult::error(vec![ContentBlock::text(error)]))
            .into())
    }
}

/// The agent's own metadata of a call, such as the session Codex names in `threadId`, which
/// the reviewer reads, without what belongs to the agent's connection to the bridge: the
/// protocol's keys and the progress token.
fn agent_meta(meta: RequestMetaObject) -> Option<RequestMetaObject> {
    let mut meta = meta;
    meta.0
        .0
        .retain(|key, _| key != "progressToken" && !key.starts_with("io.modelcontextprotocol/"));
    (!meta.0.0.is_empty()).then_some(meta)
}

#[cfg(test)]
#[path = "bridge.tests.rs"]
mod tests;
