//! Conversation traffic bypasses slow repository operations.

use super::{AgentTarget, ApplicationMessageSender, ReviewStore, Runtime, comments};

#[cfg(test)]
use super::{HerdrClient, WorkspaceId};

impl Runtime {
    pub(super) fn start_comments(
        &self,
        store: ReviewStore,
        messages: ApplicationMessageSender,
        target: AgentTarget,
        commands: std::sync::mpsc::Sender<super::WorkerCommand>,
    ) -> comments::Worker {
        comments::Worker::start(
            store,
            self.client.clone(),
            target,
            review_mcp::Endpoint::from_env(self.repository.root()),
            {
                move |event| match event {
                    comments::Event::Explore(request) => {
                        let _ = commands.send(super::WorkerCommand::ExploreMcp(Box::new(request)));
                    }
                    comments::Event::Loaded(event) => {
                        let _ = messages.send(event);
                    }
                    comments::Event::Posted(event) => {
                        let _ = messages.send(event);
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
