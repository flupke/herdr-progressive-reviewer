use super::*;

#[test]
fn legacy_answers_do_not_wake_an_idle_agent_after_reopening() {
    let mut fixture = ConversationFixture::start("codex");
    let post = Post::start(
        DiffRangeAnchor {
            source_checkpoint: "legacy".into(),
            old_path: None,
            new_path: Some("file.rs".into()),
            old_lines: None,
            new_lines: Some(0..1),
            target_kind: GuideAnchorKind::Lines,
            source_hunk_count: 1,
            old_content: None,
            new_content: None,
            diff_hash: String::new(),
        },
        "+original".into(),
        "Already answered question".into(),
    );
    let thread = post.thread_id().clone();
    fixture
        .store
        .update_threads(&"review".into(), |book| {
            book.post(post)?;
            book.post(Post::agent_reply(
                thread.clone(),
                review_threads::MessageId::parse("8d906f31-e3df-40d2-aea2-cd37df59d422")?,
                "Legacy answer without an exact boundary".into(),
            ))
        })
        .unwrap();
    fixture.reload("review");
    fixture.reopen("review");
    thread::sleep(Duration::from_millis(350));
    assert!(
        fixture.prompts().is_empty(),
        "Answered history must not wake the agent"
    );
    fixture.post("review", Post::reply(thread, "New follow-up".into()));
    fixture.wait_for_wakeups(1);
}

#[test]
fn partial_answers_and_reviewer_navigation_do_not_repeat_notifications() {
    let fixture = ConversationFixture::start("codex");
    fixture.status(AgentStatus::Working);
    let answered = fixture.new_thread("review", "first.rs", "First question");
    let pending = fixture.new_thread("review", "second.rs", "Second question");
    fixture.status(AgentStatus::Idle);
    let access = fixture.access(1);
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let client = ClientInfo::default()
                .serve(StreamableHttpClientTransport::from_uri(
                    fixture.endpoint.url(),
                ))
                .await
                .unwrap();
            fixture.status(AgentStatus::Working);
            let fetched = value(&client, "get_new_messages", json!({"review": access})).await;
            value(
                &client,
                "reply",
                json!({
                    "review": access, "thread_id": answered,
                    "message_id": "f18f77d9-3883-4b14-a0fa-9170d9d4efb4",
                    "text": "Answer to first question",
                    "in_reply_to": fetched["threads"][0]["in_reply_to"],
                }),
            )
            .await;
            fixture
                .worker
                .as_ref()
                .unwrap()
                .send(Command::ActiveAgentChanged);
            fixture.reload("review");
            fixture.status(AgentStatus::Idle);
            let remaining = value(&client, "get_new_messages", json!({"review": access})).await;
            assert_eq!(remaining["threads"].as_array().unwrap().len(), 1);
            assert_eq!(remaining["threads"][0]["thread_id"], json!(pending));
            thread::sleep(Duration::from_millis(350));
            assert_eq!(fixture.prompts().matches("Logical review: ").count(), 1);
            client.cancel().await.unwrap();
        });
}

#[test]
fn unread_work_survives_a_lost_fetch_and_reopening_until_its_exact_snapshot_is_answered() {
    let mut fixture = ConversationFixture::start("codex");
    let thread = fixture.new_thread("review", "file.rs", "First question");
    let access = fixture.access(1);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let fetched = runtime.block_on(async {
        let client = ClientInfo::default()
            .serve(StreamableHttpClientTransport::from_uri(
                fixture.endpoint.url(),
            ))
            .await
            .unwrap();
        let original = fixture.store.load_threads(&"review".into()).unwrap();
        let fetched = value(&client, "get_new_messages", json!({"review": access})).await;
        assert_eq!(
            value(
                &client,
                "get_thread",
                json!({"review": access, "thread_id": thread})
            )
            .await,
            fetched
        );
        assert_eq!(
            fixture.store.load_threads(&"review".into()).unwrap(),
            original
        );
        // The caller disconnects without acting on its response.
        client.cancel().await.unwrap();
        fetched
    });
    fixture.status(AgentStatus::Working);
    fixture.status(AgentStatus::Idle);
    thread::sleep(Duration::from_millis(250));
    assert_eq!(fixture.prompts().matches("Logical review: ").count(), 1);
    fixture.reopen("review");
    let access = fixture.access(2);
    runtime.block_on(async {
        let client = ClientInfo::default()
            .serve(StreamableHttpClientTransport::from_uri(fixture.endpoint.url()))
            .await.unwrap();
        assert_eq!(value(&client, "get_new_messages", json!({"review": access})).await, fetched);
        fixture.status(AgentStatus::Working);
        fixture.post("review", Post::reply(thread.clone(), "Posted while answering".into()));
        let reply = json!({"review": access, "thread_id": thread,
            "message_id": "75c55138-4a11-4489-b4ea-82ad4674a904", "text": "Answer to first question",
            "in_reply_to": fetched["threads"][0]["in_reply_to"]});
        assert_eq!(value(&client, "reply", reply.clone()).await, value(&client, "reply", reply.clone()).await);
        let pending = value(&client, "get_new_messages", json!({"review": access})).await;
        assert_eq!(pending["threads"][0]["messages"].as_array().unwrap().len(), 3);
        assert_ne!(pending["threads"][0]["in_reply_to"], fetched["threads"][0]["in_reply_to"]);
        assert!(fixture.store.load_threads(&"review".into()).unwrap().thread(&thread).unwrap().is_waiting());
        let mut changed_retry = reply.clone();
        changed_retry["in_reply_to"] = pending["threads"][0]["in_reply_to"].clone();
        assert_eq!(call(&client, "reply", changed_retry).await.is_error, Some(true));
        assert_eq!(value(&client, "get_new_messages", json!({"review": access})).await, pending);
        value(&client, "reply", json!({"review": access, "thread_id": thread,
            "message_id": "97061f99-9880-4bc9-89be-94f3480f7324", "text": "Answer to follow-up",
            "in_reply_to": pending["threads"][0]["in_reply_to"]})).await;
        // Retrying an older reply cannot undo acknowledgement of the newer answer.
        value(&client, "reply", reply).await;
        assert_eq!(value(&client, "get_new_messages", json!({"review": access})).await["threads"], json!([]));
        let stored = fixture.store.load_threads(&"review".into()).unwrap();
        assert_eq!(stored.thread(&thread).unwrap().messages.len(), 4);
        assert_eq!(stored.thread(&thread).unwrap().resolution, review_threads::Resolution::Open);
        client.cancel().await.unwrap();
    });
}

#[test]
fn an_interrupted_answer_can_be_retried_without_posting_another_comment_or_looping() {
    let fixture = ConversationFixture::start("codex");
    let thread = fixture.new_thread("review", "file.rs", "Retry the pending question");
    let access = fixture.access(1);
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let client = ClientInfo::default()
                .serve(StreamableHttpClientTransport::from_uri(
                    fixture.endpoint.url(),
                ))
                .await
                .unwrap();
            value(&client, "get_new_messages", json!({"review": access})).await;
            fixture.status(AgentStatus::Working);
            fixture.status(AgentStatus::Idle);
            fixture
                .worker
                .as_ref()
                .unwrap()
                .send(Command::Thread(ThreadCommand::Retry {
                    review_unit: "review".into(),
                    thread_id: thread.clone(),
                }));
            assert_eq!(fixture.access(2), access);
            let pending = value(&client, "get_new_messages", json!({"review": access})).await;
            assert_eq!(
                pending["threads"][0]["messages"].as_array().unwrap().len(),
                1
            );
            fixture.status(AgentStatus::Working);
            fixture.status(AgentStatus::Idle);
            thread::sleep(Duration::from_millis(250));
            assert_eq!(fixture.prompts().matches("Logical review: ").count(), 2);
            client.cancel().await.unwrap();
        });
}
