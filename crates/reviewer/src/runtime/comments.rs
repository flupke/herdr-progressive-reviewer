//! Conversation traffic bypasses slow repository operations.

use super::{AgentTarget, ApplicationMessageSender, ReviewStore, Runtime, comments};

#[cfg(test)]
use super::{HerdrClient, WorkspaceId};

impl Runtime {
    pub(super) fn start_comments(
        &self,
        store: ReviewStore,
        messages: ApplicationMessageSender,
    ) -> comments::Worker {
        comments::Worker::start(
            store,
            self.client.clone(),
            AgentTarget::new(self.workspace_id.clone(), self.initial_agent.clone()),
            review_mcp::Endpoint::from_env(self.repository.root()).and_then(|endpoint| {
                Ok(review_mcp_config::ProjectConfig::new(
                    self.repository.root(),
                    endpoint,
                    std::env::current_exe()
                        .map_err(|error| error.to_string())?
                        .with_file_name("reviewer-mcp"),
                ))
            }),
            {
                move |event| match event {
                    comments::Event::Loaded(event) => {
                        let _ = messages.send(event);
                    }
                    comments::Event::Posted(event) => {
                        let _ = messages.send(event);
                    }
                    comments::Event::Configured => {
                        let _ = messages.send(ui_events::ToastRequested {
                                text: "MCP configuration updated. Reload MCP or restart/resume the agent once, then check /mcp.".into(),
                                kind: toasts::ToastKind::Info,
                            });
                    }
                    comments::Event::NotificationDeferred => {
                        let _ = messages.send(ui_events::ToastRequested {
                                text: "Comments saved. Notification is waiting for the agent input to be empty and unfocused.".into(),
                                kind: toasts::ToastKind::Info,
                            });
                    }
                    comments::Event::Error(text) => {
                        let _ = messages.send(ui_events::ToastRequested {
                            text,
                            kind: toasts::ToastKind::Error,
                        });
                    }
                }
            },
        )
    }
}

#[cfg(test)]
pub(super) fn test_worker(store: &ReviewStore) -> comments::Worker {
    comments::Worker::start(
        store.clone(),
        HerdrClient::new(
            "/nonexistent/reviewer-test.sock".into(),
            "reviewer-test".into(),
            "/nonexistent".into(),
        ),
        AgentTarget::new(WorkspaceId("test".into()), None),
        Err("No MCP listener in this unit test".into()),
        |_| {},
    )
}
