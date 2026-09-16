use super::*;
use crate::runtime::{RuntimeActionDispatcher, WorkerCommand, highlighting};
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
            review_checkpoint: review_guide::ReviewCheckpoint::new("review", "now"),
            description: "Reply notification".into(),
        });
        app.publish(ui_events::RepositoryFilesChanged {
            review_checkpoint: review_guide::ReviewCheckpoint::new("review", "now"),
            files: vec![ui_events::FileSummary::new(
                "src/lib.rs",
                review_state::ReviewStatus::Reviewed,
            )],
        });
        app.publish(ui_events::ReviewThreadsLoaded {
            review_unit: "review".into(),
            result: Ok(self.store.load_threads(&"review".into()).unwrap()),
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

#[test]
fn mcp_receives_ui_posts_while_repository_work_is_pending() {
    let fixture = ConversationFixture::start("codex");
    let thread = fixture.new_thread("review", "src/lib.rs", "Initial question");
    let access = fixture.access(1);
    let (commands, pending_repository_work) = mpsc::channel();
    commands.send(WorkerCommand::Poll).unwrap();
    let (documents, _pending_documents) = mpsc::channel();
    let theme = Theme::default();
    let highlighting = highlighting::Worker::start(
        syntax_highlighting::SyntaxHighlighter::new(theme.syntax, theme.palette.text),
        |_| {},
    );
    let search = text_search::Worker::start(|_| {});
    let lsp = review_lsp::Worker::start(fixture.repository.path().to_owned());
    let dispatcher = RuntimeActionDispatcher {
        source_watches: None,
        comments: fixture.worker.as_ref().unwrap(),
        commands: &commands,
        documents: &documents,
        search: &search,
        highlighting: &highlighting,
        settings: &fixture.store,
        repository_root: fixture.repository.path(),
        lsp: &lsp,
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
            let post = Post::reply(thread.clone(), "Posted before the final fetch".into());
            dispatcher
                .dispatch(Action::Thread(ThreadCommand::Post {
                    review_unit: "review".into(),
                    post,
                }))
                .unwrap();
            // The repository queue remains undrained throughout this exchange.
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
                dispatcher.dispatch(action).unwrap();
            }
            fixture.wait_for_wakeups(2);
            let late = value(&client, "get_new_messages", json!({"review": access})).await;
            assert_eq!(
                late["threads"][0]["messages"][3]["text"],
                "Posted after the final fetch"
            );
            client.cancel().await.unwrap();
        });
    assert!(matches!(
        pending_repository_work.try_recv(),
        Ok(WorkerCommand::Poll)
    ));
    assert!(
        pending_repository_work.try_recv().is_err(),
        "thread posts bypass repository work"
    );
}
