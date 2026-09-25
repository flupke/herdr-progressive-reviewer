use std::sync::{Arc, Mutex};

use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, ClientInfo},
};
use serde_json::json;

use super::Bridge;
use crate::{Endpoint, Operation, Response, Server};

#[test]
fn one_client_keeps_its_tools_across_closed_open_and_reopened_reviewers() {
    let repository = tempfile::tempdir().unwrap();
    let lease = review_test_support::TestPort::new();
    let endpoint = Endpoint::for_repository(repository.path(), Some(lease.number())).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let (agent, bridge) = tokio::io::duplex(8192);
            let serving = tokio::spawn(async move {
                Bridge {
                    endpoint: Ok(endpoint),
                }
                .serve(bridge)
                .await
                .unwrap()
            });
            let client = ClientInfo::default().serve(agent).await.unwrap();
            let bridge = serving.await.unwrap();
            let tools = client.list_all_tools().await.unwrap();
            assert_eq!(tools.len(), 6);
            let conclusion = tools.iter().find(|tool| tool.name == "submit_conclusion").unwrap();
            for section in ["summary", "to_be_implemented", "future_work"] {
                assert_eq!(conclusion.input_schema["properties"][section]["type"], "string");
                assert!(conclusion.input_schema["required"].as_array().unwrap().iter().any(|field| field == section));
            }
            assert!(conclusion.input_schema["properties"].get("update").is_none());
            assert!(conclusion.input_schema["properties"].get("next").is_none());
            assert!(tools.iter().all(|tool| !matches!(tool.name.as_ref(), "read_explore" | "get_explore" | "get_explore_answer" | "submit_explore")));
            let calls = Arc::new(Mutex::new(Vec::new()));
            let message_id =
                review_threads::MessageId::parse("13b2c529-d4c5-4af9-8c36-8f447a9975d7").unwrap();
            let thread_id =
                serde_json::from_value::<review_threads::ThreadId>(json!("test-thread")).unwrap();
            let comment_id = review_threads::MessageId::parse("0bdf19b3-b09c-42be-92cd-5c912b81220a").unwrap();
            let request = CallToolRequestParams::new("reply").with_arguments(
                json!({
                    "review": "private-access", "thread_id": thread_id,
                    "message_id": message_id, "text": "Reply with `literal` text", "in_reply_to": comment_id
                })
                .as_object()
                .unwrap()
                .clone(),
            );
            for available in [false, true, false, true] {
                let observed = Arc::clone(&calls);
                let message_id = message_id.clone();
                let thread_id = thread_id.clone();
                let comment_id = comment_id.clone();
                let server = available.then(|| {
                    Server::start(endpoint, move |request| {
                        assert_eq!(request.access, "private-access");
                        let Operation::Reply(post) = &request.operation
                        else {
                            panic!("Expected a reply")
                        };
                        assert_eq!(post.thread_id(), &thread_id);
                        assert_eq!(post.message().id, message_id);
                        assert_eq!(post.message().in_reply_to.as_ref(), Some(&comment_id));
                        observed.lock().unwrap().push(post.message().text.clone());
                        request.respond(Ok(Response::Posted(message_id.clone())));
                        Ok(())
                    })
                    .unwrap()
                });
                let result = client.call_tool(request.clone()).await.unwrap();
                assert_eq!(result.is_error.unwrap_or(false), !available);
                assert_eq!(client.list_all_tools().await.unwrap(), tools);
                drop(server);
            }
            assert_eq!(*calls.lock().unwrap(), vec!["Reply with `literal` text"; 2]);
            client.cancel().await.unwrap();
            bridge.waiting().await.unwrap();
        });
}

#[test]
fn repository_discovery_failure_does_not_remove_the_tool_catalog() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let (agent, transport) = tokio::io::duplex(8192);
            let serving = tokio::spawn(async move {
                Bridge {
                    endpoint: Err("No repository in this directory".into()),
                }
                .serve(transport)
                .await
                .unwrap()
            });
            let client = ClientInfo::default().serve(agent).await.unwrap();
            let bridge = serving.await.unwrap();
            assert_eq!(client.list_all_tools().await.unwrap().len(), 6);
            let response = client
                .call_tool(
                    CallToolRequestParams::new("list_threads")
                        .with_arguments(json!({"review":"probe"}).as_object().unwrap().clone()),
                )
                .await
                .unwrap();
            assert_eq!(response.is_error, Some(true));
            assert!(
                serde_json::to_string(&response)
                    .unwrap()
                    .contains("No repository in this directory")
            );
            client.cancel().await.unwrap();
            bridge.waiting().await.unwrap();
        });
}
