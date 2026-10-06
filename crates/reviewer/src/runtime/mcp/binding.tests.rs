use super::*;

#[test]
fn focus_changes_and_restart_keep_completed_answers() {
    let mut fixture = ConversationFixture::start("codex");
    let completed = fixture.new_thread("review", "done.rs", "Completed question");
    let first_access = fixture.access(1);
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
            let fetched = value(&client, "get_new_messages", json!({"review": first_access})).await;
            value(
                &client,
                "reply",
                json!({"review": first_access, "thread_id": completed,
            "message_id": "d16d81ea-9cfb-4d40-bd1b-aa58ce0c2b5d", "text": "Already completed",
            "in_reply_to": fetched["threads"][0]["in_reply_to"]}),
            )
            .await;
            fixture.status(AgentStatus::Working);
            let pending = fixture.new_thread("review", "pending.rs", "Unanswered question");
            let second = fixture.second_agent();
            fixture.focus_agent(&second);
            fixture.server.run_cli(&[
                "pane",
                "focus",
                "--direction",
                "right",
                "--pane",
                &second.pane_id.0,
            ]);
            let access = fixture.second_access(1);
            let work = value(&client, "get_new_messages", json!({"review": access})).await;
            assert_eq!(work["threads"].as_array().unwrap().len(), 1);
            assert_eq!(work["threads"][0]["thread_id"], json!(pending));
            assert_eq!(
                value(&client, "list_threads", json!({"review": access})).await["threads"]
                    .as_array()
                    .unwrap()
                    .len(),
                2
            );
            assert_eq!(
                call(&client, "get_new_messages", json!({"review": first_access}))
                    .await
                    .is_error,
                Some(true)
            );
            client.cancel().await.unwrap();
            fixture.reopen("review");
            let fresh = fixture.second_access(2);
            assert_ne!(fresh, access);
            let book = fixture.store.load_threads(&"review".into()).unwrap();
            assert_eq!(
                book.new_messages()
                    .iter()
                    .map(|thread| &thread.id)
                    .collect::<Vec<_>>(),
                [&pending]
            );
            assert_eq!(book.reply_count(), 1);
        });
}

#[test]
fn resolution_stops_mcp_work_and_wakeups_but_preserves_late_answers() {
    let fixture = ConversationFixture::start("codex");
    let thread = fixture.new_thread("review", "file.rs", "First question");
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
            let fetched = value(&client, "get_new_messages", json!({"review": access})).await;
            fixture
                .worker
                .as_ref()
                .unwrap()
                .send(Command::Thread(ThreadCommand::SetResolution {
                    review_unit: "review".into(),
                    thread_id: thread.clone(),
                    resolution: review_threads::Resolution::Resolved,
                }));
            assert!(
                value(&client, "get_new_messages", json!({"review": access})).await["threads"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
            value(
                &client,
                "reply",
                json!({"review": access, "thread_id": thread,
            "message_id": "e47a71c8-120f-4a30-8903-238c81f3bf52", "text": "Late completed answer",
            "in_reply_to": fetched["threads"][0]["in_reply_to"]}),
            )
            .await;
            fixture.post(
                "review",
                Post::reply(thread.clone(), "Keep pending until unresolve".into()),
            );
            assert!(
                value(&client, "get_new_messages", json!({"review": access})).await["threads"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
            fixture.status(AgentStatus::Working);
            fixture.status(AgentStatus::Idle);
            assert_eq!(fixture.settled_wakeups(), 1);
            let book = fixture.store.load_threads(&"review".into()).unwrap();
            assert_eq!(
                book.thread(&thread).unwrap().resolution,
                review_threads::Resolution::Resolved
            );
            assert_eq!(book.reply_count(), 1);
            fixture
                .worker
                .as_ref()
                .unwrap()
                .send(Command::Thread(ThreadCommand::SetResolution {
                    review_unit: "review".into(),
                    thread_id: thread.clone(),
                    resolution: review_threads::Resolution::Open,
                }));
            assert_eq!(fixture.access(2), access);
            let pending = value(&client, "get_new_messages", json!({"review": access})).await;
            assert_eq!(
                pending["threads"][0]["messages"].as_array().unwrap().len(),
                3
            );
            client.cancel().await.unwrap();
        });
}

impl ConversationFixture {
    /// The access value of the latest of the comment notifications the second agent read, once
    /// it read `wakeups` of them.
    fn second_access(&self, wakeups: usize) -> String {
        let received = self.server.events().wait_until(
            "the second agent's comment notifications",
            |received| {
                prompts_of(&StandInRole::SecondAgent, received)
                    .iter()
                    .map(|prompt| prompt.matches("Logical review: ").count())
                    .sum::<usize>()
                    >= wakeups
            },
        );
        let prompts = prompts_of(&StandInRole::SecondAgent, &received).concat();
        let (_, tail) = prompts.rsplit_once("review access value `").unwrap();
        tail.split('`').next().unwrap().to_owned()
    }
}

#[test_case::test_case(false; "unretrieved")]
#[test_case::test_case(true; "retrieved")]
fn comments_follow_the_active_agent(retrieved: bool) {
    let fixture = ConversationFixture::start("codex");
    let first = fixture.new_thread("review", "file.rs", "Question for the first agent");
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
            if retrieved {
                value(&client, "get_new_messages", json!({"review": access})).await;
            }
            let mut agent_target = fixture.target.clone();
            let second = fixture.second_agent();
            fixture.focus_agent(&second);
            fixture.server.run_cli(&[
                "pane",
                "focus",
                "--direction",
                "right",
                "--pane",
                &second.pane_id.0,
            ]);
            assert_eq!(
                agent_target
                    .resolve(&fixture.server.client())
                    .unwrap()
                    .unwrap()
                    .pane_id,
                second.pane_id
            );
            let second_access = fixture.second_access(1);
            assert_ne!(access, second_access);
            assert_eq!(
                call(&client, "get_new_messages", json!({"review": access}))
                    .await
                    .is_error,
                Some(true)
            );
            fixture.reload("other-review");
            fixture.reload("review");
            fixture.post(
                "review",
                Post::reply(first.clone(), "Follow-up for the active agent".into()),
            );
            let updated = value(
                &client,
                "get_new_messages",
                json!({"review": second_access}),
            )
            .await;
            assert_eq!(updated["threads"].as_array().unwrap().len(), 1);
            assert_eq!(updated["threads"][0]["thread_id"], json!(first));
            assert_eq!(
                updated["threads"][0]["messages"][1]["text"],
                "Follow-up for the active agent"
            );
            fixture
                .worker
                .as_ref()
                .unwrap()
                .send(Command::Thread(ThreadCommand::Retry {
                    review_unit: "review".into(),
                    thread_id: first.clone(),
                }));
            assert_eq!(fixture.second_access(3), second_access);
            assert_eq!(fixture.settled_wakeups(), 1);
            client.cancel().await.unwrap();
        });
}

#[test]
fn changing_focus_hands_pending_comments_to_a_second_agent() {
    let fixture = ConversationFixture::start_with_session("codex", None);
    let first = fixture.new_thread("review", "file.rs", "Pending before detection");
    let first_access = fixture.access(1);
    let second = fixture.second_agent();
    fixture.focus_agent(&second);
    fixture.server.run_cli(&[
        "pane",
        "focus",
        "--direction",
        "right",
        "--pane",
        &second.pane_id.0,
    ]);
    let access = fixture.second_access(1);
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
            let updated = value(&client, "get_new_messages", json!({"review": access})).await;
            assert_eq!(updated["threads"][0]["thread_id"], json!(first));
            assert_eq!(fixture.settled_wakeups(), 1);
            assert_eq!(
                call(&client, "get_new_messages", json!({"review": first_access}))
                    .await
                    .is_error,
                Some(true)
            );
            client.cancel().await.unwrap();
        });
}
