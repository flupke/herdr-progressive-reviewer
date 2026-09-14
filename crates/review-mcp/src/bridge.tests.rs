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
    let endpoint = Endpoint::for_repository(repository.path(), None).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let (agent, bridge) = tokio::io::duplex(8192);
            let serving =
                tokio::spawn(async move { Bridge { endpoint }.serve(bridge).await.unwrap() });
            let client = ClientInfo::default().serve(agent).await.unwrap();
            let bridge = serving.await.unwrap();
            let tools = client.list_all_tools().await.unwrap();
            assert_eq!(tools.len(), 4);
            let calls = Arc::new(Mutex::new(Vec::new()));
            let message_id =
                review_threads::MessageId::parse("13b2c529-d4c5-4af9-8c36-8f447a9975d7").unwrap();
            let thread_id =
                serde_json::from_value::<review_threads::ThreadId>(json!("test-thread")).unwrap();
            let request = CallToolRequestParams::new("reply").with_arguments(
                json!({
                    "review": "private-access", "thread_id": thread_id,
                    "message_id": message_id, "text": "Reply with `literal` text"
                })
                .as_object()
                .unwrap()
                .clone(),
            );
            for available in [false, true, false, true] {
                let observed = Arc::clone(&calls);
                let message_id = message_id.clone();
                let thread_id = thread_id.clone();
                let server = available.then(|| {
                    Server::start(endpoint, move |request| {
                        assert_eq!(request.access, "private-access");
                        let Operation::Reply {
                            thread_id: received_thread,
                            message_id: received_id,
                            text,
                        } = &request.operation
                        else {
                            panic!("Expected a reply")
                        };
                        assert_eq!(*received_thread, thread_id);
                        assert_eq!(*received_id, message_id);
                        observed.lock().unwrap().push(text.clone());
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
