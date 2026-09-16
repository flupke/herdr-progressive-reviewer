use super::*;

impl ConversationFixture {
    fn replace_session(&self, session: &str) {
        self.server
            .run_cli(&["pane", "send-keys", &self.server.pane_id.0, "ctrl+d"]);
        let deadline = Instant::now() + Duration::from_secs(5);
        while self
            .server
            .client()
            .get_agent(&self.server.pane_id)
            .unwrap()
            .is_some()
        {
            assert!(
                Instant::now() < deadline,
                "the previous test agent did not exit"
            );
            thread::sleep(Duration::from_millis(25));
        }
        self.server.run_cli(&[
            "pane",
            "run",
            &self.server.pane_id.0,
            "env",
            &format!("REVIEW_GUIDE_E2E_AGENT_SESSION={session}"),
            &self.server.agent_binary.to_string_lossy(),
            "--exact",
            "runtime::tests::guide_e2e_agent_process",
            "--nocapture",
        ]);
        self.server.wait_for_agent(Some(session));
        self.server.release_agent();
    }
}

#[test_case::test_case("codex"; "codex")]
#[test_case::test_case("claude"; "claude")]
fn mcp_waits_for_the_selected_session_before_publishing_access(agent: &str) {
    let fixture = ConversationFixture::start_with_session(agent, None);
    let first = fixture.new_thread("review", "file.rs", "Question before session detection");
    thread::sleep(Duration::from_millis(350));
    assert!(
        fixture.prompts().is_empty(),
        "access was sent before the session was known"
    );
    assert_eq!(
        fixture
            .store
            .load_threads(&"review".into())
            .unwrap()
            .threads()
            .len(),
        1
    );

    // Detecting an unfocused agent must not change the active target.
    let second = fixture.second_agent();
    let worker = fixture.worker.as_ref().unwrap();
    worker.send(Command::Observe(HerdrEvent::AgentDetected {
        pane_id: second.pane_id,
        workspace_id: second.workspace_id,
        agent: second.agent,
        released: false,
        final_status: None,
    }));
    fixture.reload("other-review");
    fixture.reload("review");
    fixture.server.report_session("resumed-session");
    let access = fixture.access(1);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let client = ClientInfo::default()
            .serve(StreamableHttpClientTransport::from_uri(fixture.endpoint.url()))
            .await.unwrap();
        let updated = value(&client, "get_new_messages", json!({"review": access})).await;
        assert_eq!(updated["threads"][0]["thread_id"], json!(first));
        let reply = json!({"review": access, "thread_id": first,
            "message_id": "507f0ec2-c44e-4ac9-8382-097367b63203", "text": "Answer after session detection", "in_reply_to": updated["threads"][0]["in_reply_to"]});
        assert_eq!(value(&client, "reply", reply.clone()).await, value(&client, "reply", reply).await);
        assert!(value(&client, "get_new_messages", json!({"review": access})).await["threads"].as_array().unwrap().is_empty());
        client.cancel().await.unwrap();
    });
    let second_prompts =
        fs::read_to_string(fixture.server.directory.path().join("second-prompt.txt")).unwrap();
    assert!(second_prompts.is_empty());
    assert_eq!(fixture.prompts().matches("Logical review: ").count(), 1);
}

#[test]
fn mcp_retry_uses_the_active_session_without_repeating_a_failed_notification() {
    let fixture = ConversationFixture::start("codex");
    let first = fixture.new_thread("review", "file.rs", "Question before reconnect");
    let expired = fixture.access(1);
    fixture.status(AgentStatus::Working);
    fixture.status(AgentStatus::Idle);
    thread::sleep(Duration::from_millis(350));
    assert_eq!(
        fixture.prompts().matches("Logical review: ").count(),
        1,
        "an agent that cannot retrieve comments must not enter a notification loop"
    );

    fixture.replace_session("replacement-session");
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
        assert_eq!(
            call(&client, "get_new_messages", json!({"review": expired}))
                .await
                .is_error,
            Some(true)
        );
        fixture
            .worker
            .as_ref()
            .unwrap()
            .send(Command::Thread(ThreadCommand::Retry {
                review_unit: "review".into(),
                thread_id: first.clone(),
            }));
        let access = fixture.access(2);
        assert_ne!(access, expired);
        let updated = value(&client, "get_new_messages", json!({"review": access})).await;
        assert_eq!(updated["threads"][0]["thread_id"], json!(first));
        assert_eq!(
            call(
                &client,
                "reply",
                json!({"review": expired, "thread_id": first,
            "message_id": "7370f6d5-14aa-48cf-8a76-31b22c644fb5", "text": "Wrong session", "in_reply_to": updated["threads"][0]["in_reply_to"]})
            )
            .await
            .is_error,
            Some(true)
        );
        assert_eq!(
            fixture
                .store
                .load_threads(&"review".into())
                .unwrap()
                .reply_count(),
            0
        );
        client.cancel().await.unwrap();
    });
}
