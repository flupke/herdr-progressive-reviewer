use super::{
    GUARD, IsolatedHerdrServer, StandInCommand, StandInEvent, StandInRole, fs, mpsc, prompts_of,
};

use herdr_client::protocol::{
    AgentPort, AgentStatus, AgentTarget, HerdrEvent, HerdrReader, HerdrWriter,
};
use review_mcp::Endpoint;
use review_source::{AnchorKind, DiffRangeAnchor};
use review_store::ReviewStore;
use review_thread_service::{Command, Event, Worker};
use review_threads::{Post, ThreadCommand, ThreadId};
use rmcp::{
    Peer, RoleClient, ServiceExt,
    model::{CallToolRequestParams, CallToolResult, ClientInfo},
    transport::StreamableHttpClientTransport,
};
use serde_json::{Value, json};

#[path = "mcp/session.tests.rs"]
mod session;

#[path = "mcp/routing.tests.rs"]
mod routing;

#[path = "mcp/delivery.tests.rs"]
mod delivery;

#[path = "mcp/prompts.tests.rs"]
mod prompts;

struct ConversationFixture {
    worker: Option<Worker>,
    target: AgentTarget,
    events: mpsc::Receiver<Event>,
    store: ReviewStore,
    endpoint: Endpoint,
    server: IsolatedHerdrServer,
    repository: tempfile::TempDir,
    _port: review_test_support::TestPort,
}

impl ConversationFixture {
    fn start(agent: &str) -> Self {
        Self::start_with_session(agent, Some("session"))
    }

    fn start_with_session(agent: &str, session: Option<&str>) -> Self {
        let repository = tempfile::tempdir().unwrap();
        let server = IsolatedHerdrServer::start_with_session(repository.path(), agent, session);
        // Native status detection must own the pane after session registration.
        server.release_agent();
        server.run_cli(&[
            "pane",
            "split",
            &server.pane_id.0,
            "--direction",
            "right",
            "--no-focus",
        ]);
        server.run_cli(&[
            "pane",
            "focus",
            "--direction",
            "right",
            "--pane",
            &server.pane_id.0,
        ]);
        let port = review_test_support::TestPort::new();
        let endpoint = Endpoint::for_repository(repository.path(), Some(port.number())).unwrap();
        let store = ReviewStore::open(server.server.state_directory(), repository.path()).unwrap();
        let (sender, events) = mpsc::channel();
        let target = AgentTarget::new(server.workspace_id.clone(), Some(server.pane_id.clone()));
        let worker = Worker::start(
            ReviewStore::open(server.server.state_directory(), repository.path()).unwrap(),
            server.client(),
            target.clone(),
            Ok(endpoint),
            |_| Err("No Explore session in this test".into()),
            move |event| {
                let _ = sender.send(event);
            },
        );
        let fixture = Self {
            worker: Some(worker),
            target,
            events,
            store,
            endpoint,
            server,
            repository,
            _port: port,
        };
        // Session registration can precede native status detection. Publish the
        // fixture's idle title and wait for it before measuring notification delivery.
        fixture.status(AgentStatus::Idle);
        fixture
    }

    fn reopen(&mut self, unit: &str) -> review_threads::SavedDrafts {
        drop(self.worker.take());
        assert!(std::net::TcpStream::connect(self.endpoint.address()).is_err());
        let (sender, events) = mpsc::channel();
        self.events = events;
        self.worker = Some(Worker::start(
            self.store.clone(),
            self.server.client(),
            self.target.clone(),
            Ok(self.endpoint),
            |_| Err("No Explore session in this test".into()),
            move |event| {
                let _ = sender.send(event);
            },
        ));
        self.reload(unit)
    }

    fn focus_agent(&self, agent: &herdr_client::protocol::Agent) {
        self.server.client().focus_agent(&agent.pane_id).unwrap();
        self.target.clone().observe_focus(&agent.pane_id);
        self.worker
            .as_ref()
            .unwrap()
            .send(Command::ActiveAgentChanged);
    }

    /// The drafts restored with the review.
    fn reload(&self, unit: &str) -> review_threads::SavedDrafts {
        self.worker
            .as_ref()
            .unwrap()
            .send(Command::Thread(ThreadCommand::Load(unit.into())));
        loop {
            match self.events.recv_timeout(GUARD).unwrap() {
                Event::Loaded(event) if event.review_unit.as_str() == unit => {
                    event.result.unwrap();
                    return event.drafts;
                }
                Event::Error(error) => panic!("{error}"),
                _ => {}
            }
        }
    }

    fn access(&self, wakeups: usize) -> String {
        self.wait_for_wakeups(wakeups)
            .rsplit("review access value `")
            .next()
            .unwrap()
            .split('`')
            .next()
            .unwrap()
            .to_owned()
    }

    /// A second agent, on the session `session`, in a pane of its own, whose state Herdr
    /// reads from its title.
    fn second_agent(&self) -> herdr_client::protocol::Agent {
        let from = self.server.events().received().len();
        let mut arguments = vec![
            "pane",
            "split",
            &self.server.pane_id.0,
            "--direction",
            "right",
            "--no-focus",
            "--env",
            "REVIEW_AGENT_E2E_ROLE=second",
            "--env",
            "REVIEW_AGENT_E2E_AGENT_SESSION=session",
        ];
        for variable in &self.server.environment {
            arguments.extend(["--env", variable]);
        }
        let split = self.server.server.run_cli_json(&arguments);
        let pane = split["result"]["pane"]["pane_id"]
            .as_str()
            .unwrap_or_else(|| panic!("split: {split}"));
        self.server.run_cli(&[
            "pane",
            "run",
            pane,
            &self.server.agent_binary.to_string_lossy(),
            "--exact",
            "runtime::tests::e2e_agent_process",
            "--ignored",
            "--nocapture",
        ]);
        self.server
            .events()
            .wait_until("the second agent's session", |received| {
                received[from..].iter().any(|reported| {
                    reported.from == StandInRole::SecondAgent
                        && matches!(reported.event, StandInEvent::SessionReported { .. })
                })
            });
        // Herdr's own detection shows the turns the agent starts once it saw the agent idle.
        let pane = herdr_client::protocol::PaneId(pane.to_owned());
        self.server.run_cli(&[
            "pane",
            "release-agent",
            &pane.0,
            "--source",
            super::AGENT_E2E_AGENT_SOURCE,
            "--agent",
            &self.server.agent,
        ]);
        // Herdr says that it released an agent only when it did not detect it by name yet.
        let from = self.server.events().received().len();
        self.server
            .events()
            .send(&StandInRole::SecondAgent, &StandInCommand::Release);
        self.server
            .events()
            .wait_until("the second agent's release", |received| {
                received[from..].iter().any(|reported| {
                    reported.from == StandInRole::SecondAgent
                        && reported.event == StandInEvent::Released
                })
            });
        // A released agent that Herdr did not detect by name yet is gone until it detects it:
        // its first status says that it is back.
        let statuses = self.server.server.agent_statuses(&pane);
        self.server.events().send(
            &StandInRole::SecondAgent,
            &StandInCommand::ShowTitle {
                title: "✳ Ready".into(),
            },
        );
        statuses.wait_for(AgentStatus::Idle);
        self.server
            .client()
            .get_agent(&pane)
            .unwrap()
            .expect("the second agent")
    }

    /// The prompts the second agent read once the reviewer sent every prompt it was to send by
    /// now.
    fn settled_second_prompts(&self, second: &herdr_client::protocol::Agent) -> Vec<String> {
        self.flush_prompts();
        self.server.events().mark(|marker| {
            self.server
                .client()
                .submit_agent_command(&second.pane_id, marker)
                .unwrap();
        });
        prompts_of(&StandInRole::SecondAgent, &self.server.events().received())
    }

    /// Returns once every prompt the worker got by now is sent, withdrawn or failed.
    fn flush_prompts(&self) {
        if let Some(worker) = &self.worker {
            worker.prompt_sender().flush();
        }
    }

    fn post(&self, unit: &str, post: Post) -> ThreadId {
        let id = post.thread_id().clone();
        let message = post.message().id.clone();
        self.worker
            .as_ref()
            .unwrap()
            .send(Command::Thread(ThreadCommand::Post {
                review_unit: unit.into(),
                post,
            }));
        loop {
            match self.events.recv_timeout(GUARD).unwrap() {
                Event::Posted(event) if event.message_id == message => {
                    event.result.unwrap();
                    return id;
                }
                Event::Error(error) => panic!("{error}"),
                _ => {}
            }
        }
    }

    fn new_thread(&self, unit: &str, path: &str, text: &str) -> ThreadId {
        self.post(
            unit,
            Post::start(
                DiffRangeAnchor {
                    source_checkpoint: "original".into(),
                    old_path: None,
                    new_path: Some(path.into()),
                    old_lines: None,
                    new_lines: Some(1..2),
                    target_kind: AnchorKind::Lines,
                    source_hunk_count: 1,
                    old_content: None,
                    new_content: Some(b"original\n".to_vec()),
                    diff_hash: String::new(),
                },
                "+original".into(),
                text.into(),
            ),
        )
    }

    /// Shows the agent `status`, and waits until Herdr sees it.
    fn status(&self, status: AgentStatus) {
        if status == AgentStatus::Working {
            self.server.show_working();
        } else {
            self.server.show_idle();
        }
    }

    /// The prompts the agent read, as far as the test received its reports, each on its lines.
    fn prompts(&self) -> String {
        super::prompts_text(&self.server.prompts())
    }

    /// The prompts the agent read once the reviewer sent every prompt it was to send by now,
    /// each on its lines.
    fn settled_prompts(&self) -> String {
        self.flush_prompts();
        self.server.mark();
        self.prompts()
    }

    /// How many comment notifications the agent read once the reviewer sent every prompt it was
    /// to send by now.
    fn settled_wakeups(&self) -> usize {
        self.settled_prompts().matches("Logical review: ").count()
    }

    /// The prompts the agent read, once `count` of them were comment notifications, each on
    /// its lines.
    fn wait_for_wakeups(&self, count: usize) -> String {
        super::prompts_text(&self.server.wait_for_prompts(
            &format!("{count} comment notifications"),
            |prompts| {
                prompts
                    .iter()
                    .map(|prompt| prompt.matches("Logical review: ").count())
                    .sum::<usize>()
                    >= count
            },
        ))
    }
}

#[test]
fn mcp_startup_and_reopening_leave_project_configuration_untouched() {
    let mut fixture = ConversationFixture::start("codex");
    fixture.reload("review");
    let paths =
        [".codex/config.toml", ".mcp.json"].map(|path| fixture.repository.path().join(path));
    for path in &paths {
        assert!(!path.exists());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "Existing project configuration must stay unchanged").unwrap();
    }
    fixture.reopen("review");
    for path in paths {
        assert_eq!(
            fs::read_to_string(path).unwrap(),
            "Existing project configuration must stay unchanged"
        );
    }
}

#[test_case::test_case("codex"; "codex")]
#[test_case::test_case("claude"; "claude")]
fn multiline_prompts_are_captured_whole_only_after_submission(agent: &str) {
    let fixture = ConversationFixture::start(agent);
    let client = fixture.server.client();
    let body = "éreview ".repeat(800);
    let mut expected = Vec::new();
    for index in 0..12 {
        let prompt = format!("Prompt {index}\n\n{body}\nLast line {index}");
        client
            .prompt_agent(&fixture.server.pane_id, &prompt)
            .unwrap();
        expected.push(prompt);
        let captured = fixture
            .server
            .wait_for_prompts(&format!("prompt {index}"), |prompts| {
                prompts.len() == expected.len()
            });
        assert!(
            captured == expected,
            "Prompt {index} was corrupted or truncated"
        );
    }
}

#[test_case::test_case("codex"; "codex")]
#[test_case::test_case("claude"; "claude")]
fn mcp_posts_notify_a_working_focused_agent_without_recognizing_its_composer(agent: &str) {
    let fixture = ConversationFixture::start(agent);
    let client = fixture.server.client();
    client.focus_agent(&fixture.server.pane_id).unwrap();
    fixture.server.events().send(
        &StandInRole::Agent,
        &StandInCommand::ShowScreen {
            text: "An unfamiliar agent input layout".into(),
        },
    );
    // The agent draws its screen before the working title Herdr then reads.
    fixture.status(AgentStatus::Working);
    let screen = client.read_agent_screen(&fixture.server.pane_id).unwrap();
    assert!(
        screen.contains("An unfamiliar agent input layout"),
        "{screen:?}"
    );
    let id = fixture.new_thread("review", "src/lib.rs", "First comment");
    let access = fixture.access(1);
    assert!(
        fixture
            .store
            .load_threads(&"review".into())
            .unwrap()
            .thread(&id)
            .is_some()
    );

    fixture.post("review", Post::reply(id, "Follow-up while working".into()));
    assert_eq!(fixture.access(2), access);
    fixture.status(AgentStatus::Idle);
    fixture.reload("review");
    assert_eq!(fixture.settled_wakeups(), 2);
}

async fn call(client: &Peer<RoleClient>, tool: &'static str, arguments: Value) -> CallToolResult {
    client
        .call_tool(
            CallToolRequestParams::new(tool).with_arguments(arguments.as_object().unwrap().clone()),
        )
        .await
        .unwrap()
}

async fn value(client: &Peer<RoleClient>, tool: &'static str, arguments: Value) -> Value {
    let result = call(client, tool, arguments).await;
    assert_ne!(result.is_error, Some(true), "{result:?}");
    serde_json::from_str(&result.content[0].as_text().unwrap().text).unwrap()
}

#[test_case::test_case("codex"; "codex")]
#[test_case::test_case("claude"; "claude")]
fn mcp_threads_exchange_through_an_isolated_herdr_agent(agent: &str) {
    let mut fixture = ConversationFixture::start(agent);
    let first = fixture.new_thread("review", "gone.rs", "First question");
    let prompts = fixture.wait_for_wakeups(1);
    let access = prompts
        .split("review access value `")
        .nth(1)
        .unwrap()
        .split('`')
        .next()
        .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let client = ClientInfo::default().serve(StreamableHttpClientTransport::from_uri(fixture.endpoint.url())).await.unwrap();
        let mut names = client.list_all_tools().await.unwrap().into_iter().map(|tool| tool.name.into_owned()).collect::<Vec<_>>();
        names.sort();
        assert_eq!(names, ["get_new_messages", "get_thread", "list_threads", "reply", "submit_conclusion", "submit_question"]);
        assert_eq!(call(&client, "list_threads", json!({"review": "invalid"})).await.is_error, Some(true));
        fixture.status(AgentStatus::Working);
        let second = fixture.new_thread("review", "another.rs", "Second question");
        let listed = value(&client, "list_threads", json!({"review": access})).await;
        assert_eq!(listed["threads"].as_array().unwrap().len(), 2);
        let updated = value(&client, "get_new_messages", json!({"review": access})).await;
        assert_eq!(updated["threads"].as_array().unwrap().len(), 2);
        assert_eq!(updated["threads"][0]["code_context"], "+original");
        assert_eq!(updated["threads"][0]["source_checkpoint"], "original");
        assert!(!updated.to_string().contains("new_content"));
        assert_eq!(fixture.wait_for_wakeups(2).matches("Logical review: ").count(), 2);

        let reply = json!({"review": access, "thread_id": first, "message_id": "d2b82f52-39d7-4a37-a6ca-28d6f8be17db", "text": "Agent answer", "in_reply_to": updated["threads"][0]["in_reply_to"]});
        assert_eq!(value(&client, "reply", reply.clone()).await, value(&client, "reply", reply).await);
        value(&client, "reply", json!({"review": access, "thread_id": second, "message_id": "8a1ee0a6-9c99-43a6-8127-97a5e18bf139", "text": "Second answer", "in_reply_to": updated["threads"][1]["in_reply_to"]})).await;
        fixture.worker.as_ref().unwrap().send(Command::Thread(ThreadCommand::MarkRepliesRead {
            review_unit: "review".into(),
            messages: vec![review_threads::MessageId::parse("d2b82f52-39d7-4a37-a6ca-28d6f8be17db").unwrap()],
        }));
        value(&client, "list_threads", json!({"review": access})).await;
        assert!(!fixture.store.load_threads(&"review".into()).unwrap().thread(&first).unwrap().has_unread_replies());
        fixture.post("review", Post::reply(first.clone(), "One more detail".into()));
        let updated = value(&client, "get_new_messages", json!({"review": access})).await;
        assert_eq!(updated["threads"].as_array().unwrap().len(), 1);
        let messages = updated["threads"][0]["messages"].as_array().unwrap();
        assert_eq!(messages.iter().map(|m| m["text"].as_str().unwrap()).collect::<Vec<_>>(), ["First question", "Agent answer", "One more detail"]);
        assert_eq!(messages[1]["author"], "agent");
        value(&client, "reply", json!({"review": access, "thread_id": first, "message_id": "4e5c6c51-606c-4f26-8a01-7875cf4c1a40", "text": "Follow-up answer", "in_reply_to": updated["threads"][0]["in_reply_to"]})).await;
        assert!(value(&client, "get_new_messages", json!({"review": access})).await["threads"].as_array().unwrap().is_empty());

        fixture.post("review", Post::reply(first.clone(), "After the final read".into()));
        fixture.status(AgentStatus::Idle);
        fixture.wait_for_wakeups(4);
        value(&client, "get_new_messages", json!({"review": access})).await;
        assert_eq!(fixture.settled_wakeups(), 4);

        fixture.status(AgentStatus::Working);
        let other = fixture.new_thread("other-review", "private.rs", "Different logical review");
        let listed = value(&client, "list_threads", json!({"review": access})).await;
        assert_eq!(listed["threads"].as_array().unwrap().len(), 2);
        assert_eq!(call(&client, "get_thread", json!({"review": access, "thread_id": other})).await.is_error, Some(true));
        assert_eq!(value(&client, "get_thread", json!({"review": access, "thread_id": second})).await["threads"][0]["path"], "another.rs");
        let persisted = fixture.store.load_threads(&"review".into()).unwrap();
        assert_eq!(persisted.thread(&first).unwrap().messages.len(), 5);
        assert_eq!(persisted.reply_count(), 3);
        fixture.server.release_agent();
        fixture.worker.as_ref().unwrap().send(Command::Observe(HerdrEvent::AgentDetected {
            pane_id: fixture.server.pane_id.clone(), workspace_id: fixture.server.workspace_id.clone(),
            agent: Some(agent.into()), released: true, final_status: None,
        }));
        // A release event can arrive after the same native session has resumed.
        value(&client, "list_threads", json!({"review": access})).await;
        client.cancel().await.unwrap();
    });
    drop(fixture.worker.take());
    assert!(std::net::TcpStream::connect(fixture.endpoint.address()).is_err());
}

#[test_case::test_case("codex"; "codex")]
#[test_case::test_case("claude"; "claude")]
fn mcp_reopening_sends_fresh_access_only_for_unread_comments(agent: &str) {
    let mut fixture = ConversationFixture::start(agent);
    let first = fixture.new_thread("review", "file.rs", "Question before setup");
    let expired = fixture.access(1);
    fixture.status(AgentStatus::Working);
    fixture.reopen("review");
    let access = fixture.access(2);
    assert_ne!(access, expired);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let client = ClientInfo::default().serve(StreamableHttpClientTransport::from_uri(fixture.endpoint.url())).await.unwrap();
        assert_eq!(call(&client, "get_new_messages", json!({"review": expired})).await.is_error, Some(true));
        fixture.status(AgentStatus::Working);
        let updated = value(&client, "get_new_messages", json!({"review": access})).await;
        assert_eq!(updated["threads"][0]["thread_id"], json!(first));
        value(&client, "reply", json!({"review": access, "thread_id": first, "message_id": "7b3e9e91-334b-410d-96e9-71c0d82f742a", "text": "Answer after reconnect", "in_reply_to": updated["threads"][0]["in_reply_to"]})).await;
        client.cancel().await.unwrap();
    });
    fixture.reopen("review");
    fixture.status(AgentStatus::Idle);
    assert_eq!(
        fixture.settled_wakeups(),
        2,
        "already answered comments must not restart the agent"
    );
    assert_eq!(
        fixture
            .store
            .load_threads(&"review".into())
            .unwrap()
            .reply_count(),
        1
    );
}

#[test]
fn drafts_are_restored_after_reopening_until_they_are_posted() {
    let mut fixture = ConversationFixture::start("codex");
    let mut draft = review_threads::Draft::start(
        "draft.rs".into(),
        std::sync::Arc::new(review_threads::ThreadSource {
            anchor: DiffRangeAnchor {
                source_checkpoint: "original".into(),
                old_path: None,
                new_path: Some("draft.rs".into()),
                old_lines: None,
                new_lines: Some(1..2),
                target_kind: AnchorKind::Lines,
                source_hunk_count: 1,
                old_content: None,
                new_content: Some(b"original\n".to_vec()),
                diff_hash: String::new(),
            },
            excerpt: "+original".into(),
        }),
    );
    draft.text = "Still composing".into();
    fixture
        .worker
        .as_ref()
        .unwrap()
        .send(Command::Thread(ThreadCommand::SaveDraft {
            review_unit: "review".into(),
            draft: draft.clone(),
        }));
    assert_eq!(fixture.reopen("review").drafts(), [draft.clone()]);
    assert!(
        fixture
            .store
            .load_threads(&"review".into())
            .unwrap()
            .threads()
            .is_empty()
    );
    fixture.post("review", draft.post());
    assert!(
        fixture
            .store
            .load_drafts(&"review".into())
            .unwrap()
            .is_empty()
    );
    assert!(fixture.reopen("review").is_empty());
}

#[test]
fn mcp_agent_detection_does_not_reassign_retrieved_comments() {
    let fixture = ConversationFixture::start("codex");
    fixture.new_thread("review", "file.rs", "Question for the first agent");
    let access = fixture.access(1);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let client = ClientInfo::default()
            .serve(StreamableHttpClientTransport::from_uri(
                fixture.endpoint.url(),
            ))
            .await
            .unwrap();
        value(&client, "get_new_messages", json!({"review": access})).await;
        let second = fixture.second_agent();
        let worker = fixture.worker.as_ref().unwrap();
        worker.send(Command::Observe(HerdrEvent::AgentDetected {
            pane_id: second.pane_id.clone(),
            workspace_id: second.workspace_id.clone(),
            agent: second.agent.clone(),
            released: false,
            final_status: None,
        }));
        // Round-trip through the worker, then let any wakeup it sent arrive.
        value(&client, "list_threads", json!({"review": access})).await;
        let prompts = fixture.settled_second_prompts(&second);
        assert!(
            prompts.is_empty(),
            "agent detection reassigned old comments: {prompts:?}"
        );
        assert_eq!(
            value(&client, "get_new_messages", json!({"review": access})).await["threads"]
                .as_array()
                .unwrap()
                .len(),
            1,
            "reading must not consume the original agent's pending work"
        );
        client.cancel().await.unwrap();
    });
}

#[path = "mcp/binding.tests.rs"]
mod binding;
