use super::{IsolatedHerdrServer, fs, mpsc, thread};
use std::time::{Duration, Instant};

use herdr_client::protocol::{
    AgentPrompter, AgentStatus, AgentTarget, HerdrEvent, HerdrReader, HerdrWriter,
};
use review_guide::{DiffRangeAnchor, GuideAnchorKind};
use review_mcp::Endpoint;
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
        server.wait_for_agent(session);
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
        let store = ReviewStore::open(&server.state_directory, repository.path()).unwrap();
        let (sender, events) = mpsc::channel();
        let target = AgentTarget::new(server.workspace_id.clone(), Some(server.pane_id.clone()));
        let worker = Worker::start(
            ReviewStore::open(&server.state_directory, repository.path()).unwrap(),
            server.client(),
            target.clone(),
            Ok(endpoint),
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

    fn reopen(&mut self, unit: &str) {
        drop(self.worker.take());
        assert!(std::net::TcpStream::connect(self.endpoint.address()).is_err());
        let (sender, events) = mpsc::channel();
        self.events = events;
        self.worker = Some(Worker::start(
            self.store.clone(),
            self.server.client(),
            self.target.clone(),
            Ok(self.endpoint),
            move |event| {
                let _ = sender.send(event);
            },
        ));
        self.reload(unit);
    }

    fn focus_agent(&self, agent: &herdr_client::protocol::Agent) {
        self.server.client().focus_agent(&agent.pane_id).unwrap();
        self.target.clone().observe_focus(&agent.pane_id);
        self.worker
            .as_ref()
            .unwrap()
            .send(Command::ActiveAgentChanged);
    }

    fn reload(&self, unit: &str) {
        self.worker
            .as_ref()
            .unwrap()
            .send(Command::Thread(ThreadCommand::Load(unit.into())));
        loop {
            match self.events.recv_timeout(Duration::from_secs(5)).unwrap() {
                Event::Loaded(event) if event.review_unit.as_str() == unit => {
                    event.result.unwrap();
                    return;
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

    fn second_agent(&self) -> herdr_client::protocol::Agent {
        let prompt_path = self.server.directory.path().join("second-prompt.txt");
        let split = IsolatedHerdrServer::run_cli_json_with(
            &self.server.binary,
            &self.server.socket_path,
            &[
                "pane",
                "split",
                &self.server.pane_id.0,
                "--direction",
                "right",
                "--no-focus",
                "--env",
                &format!("REVIEW_GUIDE_E2E_PROMPT_PATH={}", prompt_path.display()),
                "--env",
                &format!(
                    "REVIEW_GUIDE_E2E_HERDR_BIN={}",
                    self.server.binary.display()
                ),
                "--env",
                &format!("REVIEW_GUIDE_E2E_AGENT={}", self.server.agent),
                "--env",
                "REVIEW_GUIDE_E2E_AGENT_SESSION=session",
            ],
        );
        let pane = split["result"]["pane"]["pane_id"]
            .as_str()
            .unwrap_or_else(|| panic!("split: {split}"));
        self.server.run_cli(&[
            "pane",
            "run",
            pane,
            &self.server.agent_binary.to_string_lossy(),
            "--exact",
            "runtime::tests::guide_e2e_agent_process",
            "--nocapture",
        ]);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(agent) = self
                .server
                .client()
                .list_agents()
                .unwrap()
                .into_iter()
                .find(|agent| agent.pane_id.0 == pane && agent.agent_session.is_some())
            {
                return agent;
            }
            assert!(
                Instant::now() < deadline,
                "the second agent did not register"
            );
            thread::sleep(Duration::from_millis(25));
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
            match self.events.recv_timeout(Duration::from_secs(5)).unwrap() {
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
                    target_kind: GuideAnchorKind::Lines,
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

    fn status(&self, status: AgentStatus) {
        let title = if status == AgentStatus::Working {
            "⠋ Working"
        } else {
            "✳ Ready"
        };
        fs::write(self.server.directory.path().join("prompt.state"), title).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if self
                .server
                .client()
                .get_agent(&self.server.pane_id)
                .unwrap()
                .unwrap()
                .agent_status
                == status
            {
                break;
            }
            assert!(Instant::now() < deadline, "Herdr did not detect {status:?}");
            thread::sleep(Duration::from_millis(25));
        }
    }

    fn prompts(&self) -> String {
        fs::read_to_string(self.server.directory.path().join("prompt.txt")).unwrap_or_default()
    }

    fn wait_for_screen(&self, matches: impl Fn(&str) -> bool) -> String {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let screen = self
                .server
                .client()
                .read_agent_screen(&self.server.pane_id)
                .unwrap();
            if matches(&screen) {
                return screen;
            }
            assert!(
                Instant::now() < deadline,
                "unexpected agent screen: {screen:?}"
            );
            thread::sleep(Duration::from_millis(25));
        }
    }

    fn wait_for_wakeups(&self, count: usize) -> String {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            let prompts = self.prompts();
            if prompts.matches("Logical review: ").count() >= count {
                return prompts;
            }
            thread::sleep(Duration::from_millis(25));
        }
        panic!(
            "expected {count} wakeups, got: {}; agent screen: {:?}; agent: {:?}",
            self.prompts(),
            self.server.client().read_agent_screen(&self.server.pane_id),
            self.server.client().get_agent(&self.server.pane_id),
        );
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
    let mut expected = String::new();
    for index in 0..12 {
        let prompt = format!("Prompt {index}\n\n{body}\nLast line {index}");
        client
            .prompt_agent(&fixture.server.pane_id, &prompt)
            .unwrap();
        expected.push_str(&prompt);
        expected.push('\n');
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let captured = fixture.prompts();
            if captured == expected {
                break;
            }
            assert!(
                expected.starts_with(&captured),
                "Prompt {index} was corrupted"
            );
            assert!(
                Instant::now() < deadline,
                "Prompt {index} was truncated: captured {} of {} bytes",
                captured.len(),
                expected.len()
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
}

#[test_case::test_case("codex"; "codex")]
#[test_case::test_case("claude"; "claude")]
fn mcp_posts_notify_a_working_focused_agent_without_recognizing_its_composer(agent: &str) {
    let fixture = ConversationFixture::start(agent);
    let client = fixture.server.client();
    fixture.status(AgentStatus::Working);
    client.focus_agent(&fixture.server.pane_id).unwrap();
    fs::write(
        fixture.server.directory.path().join("prompt.screen"),
        "An unfamiliar agent input layout",
    )
    .unwrap();
    fixture.wait_for_screen(|screen| screen.contains("An unfamiliar agent input layout"));
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
    thread::sleep(Duration::from_millis(250));
    assert_eq!(fixture.prompts().matches("Logical review: ").count(), 2);
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
        thread::sleep(Duration::from_millis(250));
        assert_eq!(fixture.prompts().matches("Logical review: ").count(), 4);

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
    thread::sleep(Duration::from_millis(250));
    assert_eq!(
        fixture.prompts().matches("Logical review: ").count(),
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
            pane_id: second.pane_id,
            workspace_id: second.workspace_id,
            agent: second.agent,
            released: false,
            final_status: None,
        }));
        // Round-trip through the worker, then give an incorrect idle wakeup time to arrive.
        value(&client, "list_threads", json!({"review": access})).await;
        thread::sleep(Duration::from_millis(350));
        let prompt =
            fs::read_to_string(fixture.server.directory.path().join("second-prompt.txt")).unwrap();
        assert!(
            prompt.is_empty(),
            "agent detection reassigned old comments: {prompt}"
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
