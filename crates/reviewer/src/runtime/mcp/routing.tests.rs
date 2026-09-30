use super::*;
use review_ui::{Action, Theme};

impl ConversationFixture {
    fn reply_from_threads(&self, text: &str) -> Vec<Action> {
        use review_ui::{Key, ReviewApplication, UserInput};
        let mut app = ReviewApplication::new(Theme::default(), None, self.repository.path().into());
        app.update(UserInput::Resize {
            width: 110,
            height: 35,
        });
        app.publish(ui_events::RepositoryMetadataChanged {
            display_id: "review".into(),
            review_checkpoint: review_source::ReviewCheckpoint::new("review", "now"),
            description: "Reply notification".into(),
        });
        app.publish(ui_events::RepositoryFilesChanged {
            review_checkpoint: review_source::ReviewCheckpoint::new("review", "now"),
            files: vec![ui_events::FileSummary::new(
                "src/lib.rs",
                review_state::ReviewStatus::Reviewed,
            )],
        });
        app.publish(ui_events::ReviewThreadsLoaded {
            review_unit: "review".into(),
            result: Ok(self.store.load_threads(&"review".into()).unwrap()),
            drafts: review_threads::SavedDrafts::default(),
        });
        app.update(UserInput::Key(Key::Char('t')));
        app.update(UserInput::Key(Key::Enter));
        app.update(UserInput::Key(Key::Char('a')));
        app.update(UserInput::Paste(text.into()));
        let actions = app.update(UserInput::Key(Key::ControlEnter));
        assert!(
            matches!(
                actions.as_slice(),
                [Action::Thread(ThreadCommand::Post { .. })]
            ),
            "{actions:?}"
        );
        actions
    }
}

/// Posts from the Threads view reach the agent's next fetch.
#[test]
fn mcp_receives_ui_posts_before_and_after_the_final_fetch() {
    let fixture = ConversationFixture::start("codex");
    let thread = fixture.new_thread("review", "src/lib.rs", "Initial question");
    let access = fixture.access(1);
    let post = |action: Action| match action {
        Action::Thread(command) => fixture
            .worker
            .as_ref()
            .unwrap()
            .send(Command::Thread(command)),
        action => panic!("unexpected action {action:?}"),
    };
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
            let initial = value(&client, "get_new_messages", json!({"review": access})).await;
            assert_eq!(initial["threads"].as_array().unwrap().len(), 1);
            let reply = Post::reply(thread.clone(), "Posted before the final fetch".into());
            post(Action::Thread(ThreadCommand::Post {
                review_unit: "review".into(),
                post: reply,
            }));
            let updated = value(&client, "get_new_messages", json!({"review": access})).await;
            assert_eq!(
                updated["threads"][0]["messages"][1]["text"],
                "Posted before the final fetch"
            );
            assert_eq!(updated["threads"][0]["code_context"], "+original");
            value(&client, "reply", json!({
                "review": access, "thread_id": thread,
                "message_id": "a4e1528e-1ef2-48e8-8c8e-2bcd80d421cf", "text": "Answer to both comments",
                "in_reply_to": updated["threads"][0]["in_reply_to"]
            })).await;
            let empty = value(&client, "get_new_messages", json!({"review": access})).await;
            assert_eq!(empty["threads"], json!([]));
            fixture.status(AgentStatus::Idle);
            for action in fixture.reply_from_threads("Posted after the final fetch") {
                post(action);
            }
            fixture.wait_for_wakeups(2);
            let late = value(&client, "get_new_messages", json!({"review": access})).await;
            assert_eq!(
                late["threads"][0]["messages"][3]["text"],
                "Posted after the final fetch"
            );
            client.cancel().await.unwrap();
        });
}
