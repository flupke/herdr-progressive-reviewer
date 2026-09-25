use super::*;
use herdr_client::protocol::AgentStatus;
use review_explore::{DispatchState, ExplorePass, ViewSave};

impl ExploreFlow {
    fn store(&self) -> ReviewStore {
        ReviewStore::open(
            self.fixture.state_directory.path(),
            self.fixture.repository.root(),
        )
        .unwrap()
    }

    fn saved(&self) -> ExplorePass {
        self.store()
            .load_explore(&self.fixture.review_unit, &self.exploration.instance)
            .unwrap()
            .unwrap()
    }

    /// Restart the real worker and MCP listener, retaining only disk state and the agent.
    fn reopen(&mut self) -> ui_events::ExploreRestored {
        self.fixture.commands.send(WorkerCommand::Quit).unwrap();
        std::mem::replace(&mut self.fixture.worker_thread, thread::spawn(|| {}))
            .join()
            .unwrap();
        let store = self.store();
        let placeholder = crate::runtime::comment_service::test_worker(&store);
        drop(std::mem::replace(&mut self.fixture.comments, placeholder));
        let (commands, command_receiver) = mpsc::channel();
        let route = commands.clone();
        let comments = comments::Worker::start(
            store.clone(),
            self.fixture.herdr.client(),
            AgentTarget::new(
                self.fixture.herdr.workspace_id.clone(),
                Some(self.fixture.herdr.pane_id.clone()),
            ),
            Ok(self.endpoint),
            move |event| {
                if let comments::Event::Explore(request) = event {
                    let _ = route.send(WorkerCommand::ExploreMcp(Box::new(request)));
                }
            },
        );
        let mut worker = Worker {
            repository: self.fixture.repository.clone(),
            tracker: Arc::new(ReviewTracker::new(
                self.fixture.repository.clone(),
                store.clone(),
            )),
            guide_store: store,
            client: self.fixture.herdr.client(),
            target: AgentTarget::new(
                self.fixture.herdr.workspace_id.clone(),
                Some(self.fixture.herdr.pane_id.clone()),
            ),
            snapshot: None,
            commands: commands.clone(),
            guide: guide::GuideRequestCoordinator::default(),
            explore: explore::ExploreRuntime::default(),
            prompts: comments.prompt_sender(),
            documents: mpsc::channel().0,
        };
        let (messages, receiver) = application_message_channel();
        self.fixture.worker_thread =
            thread::spawn(move || worker.run(&command_receiver, &messages));
        self.fixture.commands = commands;
        self.fixture.messages = receiver;
        self.fixture.comments = comments;
        self.fixture.commands.send(WorkerCommand::Poll).unwrap();
        loop {
            let event = self
                .fixture
                .messages
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
            if let Some(restored) = event.downcast_ref::<ui_events::ExploreRestored>() {
                return restored.clone();
            }
        }
    }

    pub(super) fn native_status(&self, status: AgentStatus) {
        self.fixture.herdr.release_agent();
        fs::write(
            self.fixture.herdr.directory.path().join("prompt.state"),
            if status == AgentStatus::Working {
                "⠋ Working"
            } else {
                "✳ Ready"
            },
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let current = self
                .fixture
                .herdr
                .client()
                .get_agent(&self.fixture.herdr.pane_id)
                .unwrap()
                .unwrap();
            if current.agent_status == status {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "Native lifecycle did not become {status:?}: {current:?}"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }
}

#[test]
fn missing_saved_pass_rejects_mcp_then_clears_explore_on_reopen() {
    let mut flow = ExploreFlow::start(RepoType::Git);
    flow.fixture
        .herdr
        .report_session("conclusion-native-session");
    flow.turn(None, 1);
    let conclusion = flow.conclude();
    let directory = fs::read_dir(flow.store().explore_directory())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let path = directory.join(format!("{}.json", flow.exploration.instance));
    let backup = path.with_extension("retained");
    fs::rename(&path, &backup).unwrap();
    let result = flow.call("submit_conclusion", conclusion);
    assert_eq!(result.is_error, Some(true));
    assert!(
        result.content[0]
            .as_text()
            .unwrap()
            .text
            .contains("missing")
    );
    assert!(!path.exists());
    assert!(matches!(flow.reopen().result, Ok(None)));
    assert!(!path.exists());
    assert!(backup.exists());
    assert!(
        flow.store()
            .load_explore_history(&flow.fixture.review_unit)
            .unwrap()
            .passes
            .is_empty()
    );
    let toast = flow
        .fixture
        .messages
        .recv_timeout(Duration::from_secs(10))
        .unwrap();
    let toast = toast.downcast_ref::<ui_events::ToastRequested>().unwrap();
    assert_eq!(toast.kind, toasts::ToastKind::Error);
    assert!(toast.text.contains("Unreadable Explore state was cleared"));
    flow.finish();
}

#[test]
fn unreadable_index_is_rebuilt_from_intact_passes() {
    let mut flow = ExploreFlow::start(RepoType::Git);
    flow.turn(None, 1);
    let directory = fs::read_dir(flow.store().explore_directory())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    fs::write(directory.join("index.json"), b"invalid index").unwrap();

    let restored = flow.reopen();

    assert!(matches!(restored.result, Ok(Some(_))));
    assert!(restored.historical);
    assert_eq!(
        flow.store()
            .load_explore_history(&flow.fixture.review_unit)
            .unwrap()
            .passes,
        vec![flow.exploration.instance.clone()]
    );
    let toast = flow
        .fixture
        .messages
        .recv_timeout(Duration::from_secs(10))
        .unwrap();
    assert!(toast.downcast_ref::<ui_events::ToastRequested>().is_some());
    flow.finish();
}

#[test]
fn invalid_saved_pass_is_removed_and_the_ui_can_start_again() {
    let mut flow = ExploreFlow::start(RepoType::Git);
    flow.turn(None, 1);
    let directory = fs::read_dir(flow.store().explore_directory())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let path = directory.join(format!("{}.json", flow.exploration.instance));
    let mut stored: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let evidence = stored
        .pointer_mut("/value/exploration/conversation/0/update/next/evidence/0")
        .and_then(serde_json::Value::as_object_mut)
        .expect("stored question evidence");
    assert!(evidence.remove("notes").is_some());
    fs::write(&path, serde_json::to_vec(&stored).unwrap()).unwrap();
    let restored = flow.reopen();
    assert!(matches!(restored.result, Ok(None)));
    assert!(
        flow.store()
            .load_explore_history(&flow.fixture.review_unit)
            .unwrap()
            .passes
            .is_empty()
    );
    assert!(!path.exists());
    let toast = flow
        .fixture
        .messages
        .recv_timeout(Duration::from_secs(10))
        .unwrap();
    assert!(toast.downcast_ref::<ui_events::ToastRequested>().is_some());
    flow.fixture
        .commands
        .send(WorkerCommand::Explore(ExploreCommand::Start))
        .unwrap();
    let captured = loop {
        let event = flow
            .fixture
            .messages
            .recv_timeout(Duration::from_secs(10))
            .unwrap();
        if let Some(captured) = event.downcast_ref::<ui_events::ExploreCaptured>() {
            break captured.clone();
        }
    };
    assert!(captured.result.is_ok());
    flow.finish();
}

#[test]
fn unreadable_latest_pass_keeps_earlier_interview_history() {
    let mut flow = ExploreFlow::start(RepoType::Git);
    flow.turn(None, 1);
    let earlier = flow.exploration.instance.clone();
    let store = flow.store();
    let next = ExplorePass::new(Exploration::new(
        flow.saved().exploration.comparison.clone(),
    ));
    let unreadable = next.exploration.instance.clone();
    store.create_explore(next).unwrap();
    let directory = fs::read_dir(store.explore_directory())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let path = directory.join(format!("{unreadable}.json"));
    fs::write(&path, b"invalid saved pass").unwrap();

    let restored = flow.reopen();
    assert!(matches!(restored.result, Ok(Some(_))));
    assert!(restored.historical);
    assert!(!path.exists());
    assert_eq!(
        store
            .load_explore_history(&flow.fixture.review_unit)
            .unwrap()
            .passes,
        vec![earlier.clone()]
    );
    assert!(
        store
            .load_explore(&flow.fixture.review_unit, &earlier)
            .unwrap()
            .is_some()
    );
    flow.finish();
}

#[test]
fn invalid_editor_view_is_removed_without_erasing_the_interview() {
    let mut flow = ExploreFlow::start(RepoType::Git);
    flow.turn(None, 1);
    let directory = fs::read_dir(flow.store().explore_directory())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let view_path = directory.join(format!("{}.view.json", flow.exploration.instance));
    fs::write(&view_path, b"invalid editor view").unwrap();

    let restored = flow.reopen();
    assert!(matches!(restored.result, Ok(Some(_))));
    assert!(restored.view.is_none());
    assert!(restored.storage_error.is_none());
    assert!(!view_path.exists());
    assert_eq!(
        flow.store()
            .load_explore_history(&flow.fixture.review_unit)
            .unwrap()
            .passes,
        vec![flow.exploration.instance.clone()]
    );
    let toast = flow
        .fixture
        .messages
        .recv_timeout(Duration::from_secs(10))
        .unwrap();
    assert!(toast.downcast_ref::<ui_events::ToastRequested>().is_some());
    flow.finish();
}

#[test]
fn same_conversation_restores_without_a_prompt_and_retry_keeps_posted_answer_identity() {
    let mut flow = ExploreFlow::start(RepoType::Git);
    flow.fixture.herdr.report_session("persistent-conversation");
    flow.turn(None, 1);
    let question = flow.exploration.questions[0].clone();
    let request = flow
        .exploration
        .request(
            Some(AnswerInput {
                option: Some("keep".into()),
                text: "Retain this exact comment\nand line break".into(),
                ..Default::default()
            }),
            Some(&question),
        )
        .unwrap();
    flow.fixture
        .commands
        .send(WorkerCommand::Explore(ExploreCommand::Turn(Box::new(
            request.clone(),
        ))))
        .unwrap();
    flow.wait_for_prompt(&request);
    let before = fs::read(flow.fixture.herdr.directory.path().join("prompt.txt")).unwrap();
    let restored = flow.reopen();
    let pass = restored.result.unwrap().unwrap();
    assert_eq!(
        pass.exploration.answers,
        vec![request.answer.clone().unwrap()]
    );
    assert_eq!(
        pass.exploration.pending_request().unwrap().request,
        request.request
    );
    assert_eq!(
        fs::read(flow.fixture.herdr.directory.path().join("prompt.txt")).unwrap(),
        before
    );
    flow.exploration = pass.exploration.clone();
    flow.exploration.pause_delivery();
    let retry = flow.exploration.retry().unwrap();
    assert_eq!(retry, request);
    let obsolete = flow.access.clone();
    flow.fixture
        .commands
        .send(WorkerCommand::Explore(ExploreCommand::Turn(Box::new(
            retry.clone(),
        ))))
        .unwrap();
    flow.wait_for_prompt(&retry);
    assert_ne!(flow.access, obsolete);
    assert_eq!(flow.saved().exploration.answers.len(), 1);
    flow.finish();
}

#[test]
fn conclusion_edits_survive_restart_and_old_access_is_rejected() {
    let mut flow = ExploreFlow::start(RepoType::Git);
    flow.turn(None, 1);
    let conclusion = flow.conclude();
    let pass = flow.saved();
    let id = pass.exploration.conclusion_request().unwrap();
    let view = ViewSave {
        review_unit: pass.exploration.comparison.checkpoint.review_unit.clone(),
        instance: pass.exploration.instance.clone(),
        sequence: 2,
        state: review_explore::ExploreViewState {
            page: review_explore::ExplorePage::Conclusion(id.into()),
            tasks: std::collections::BTreeMap::from([(
                id.into(),
                review_types::TextEditorState {
                    text: "Only the edited task".into(),
                    ..Default::default()
                },
            )]),
            drafts: vec![(
                review_explore::ExplorePage::Conclusion(id.into()),
                review_explore::ExploreDraft {
                    editor: review_types::TextEditorState {
                        text: "Independent unposted question".into(),
                        ..Default::default()
                    },
                    correction: None,
                },
            )],
            ..Default::default()
        },
    };
    flow.store()
        .save_explore_view(&flow.fixture.review_unit, &view)
        .unwrap();
    let before = fs::read(flow.fixture.herdr.directory.path().join("prompt.txt")).unwrap();
    let restored = flow.reopen();
    assert_eq!(restored.view, Some(view.clone()));
    assert_eq!(
        restored.result.unwrap().unwrap().exploration.conversation,
        pass.exploration.conversation
    );
    assert_eq!(
        flow.call("submit_conclusion", conclusion).is_error,
        Some(true),
        "access must be renewed, not restored from disk"
    );
    assert_eq!(
        fs::read(flow.fixture.herdr.directory.path().join("prompt.txt")).unwrap(),
        before
    );
    let mut edited = view;
    edited.sequence += 1;
    edited.state.tasks.get_mut(id).unwrap().text = "Edited after reopening".into();
    flow.store()
        .save_explore_view(&flow.fixture.review_unit, &edited)
        .unwrap();
    assert_eq!(flow.reopen().view, Some(edited));
    assert_eq!(
        fs::read(flow.fixture.herdr.directory.path().join("prompt.txt")).unwrap(),
        before
    );
    flow.finish();
}

#[test]
fn implementation_crash_boundaries_keep_authorized_scope_and_never_replay_on_restore() {
    let mut flow = ExploreFlow::start(RepoType::Git);
    flow.turn(None, 1);
    flow.conclude();
    let request = flow
        .exploration
        .implementation("Exactly this authorized text".into())
        .unwrap();
    let before = fs::read(flow.fixture.herdr.directory.path().join("prompt.txt")).unwrap();
    let store = flow.store();
    store
        .update_explore(&flow.fixture.review_unit, &request.instance, |pass| {
            pass.authorize(&request).map_err(|e| e.to_string())
        })
        .unwrap();
    for state in [
        DispatchState::Queued,
        DispatchState::Attempting,
        DispatchState::Delivered,
    ] {
        store
            .update_explore(&flow.fixture.review_unit, &request.instance, |pass| {
                pass.implementations
                    .get_mut(&request.delivery)
                    .unwrap()
                    .state = state.clone();
                Ok(())
            })
            .unwrap();
        let restored = flow.reopen().result.unwrap().unwrap();
        let delivery = &restored.implementations[&request.delivery];
        assert_eq!(delivery.request.text, "Exactly this authorized text");
        assert_eq!(
            delivery.state.recovered(),
            if state == DispatchState::Attempting {
                DispatchState::Unknown
            } else {
                state
            }
        );
        assert_eq!(
            fs::read(flow.fixture.herdr.directory.path().join("prompt.txt")).unwrap(),
            before
        );
    }
    flow.finish();
}

#[test]
fn lost_ui_ack_is_durable_and_identical_retry_with_fresh_access_does_not_append() {
    let mut flow = ExploreFlow::start(RepoType::Git);
    flow.turn(None, 1);
    // Lose the acknowledgement for a conclusion while the real MCP connection is active.
    flow.fixture.herdr.report_session("ack-loss-session");
    let question = flow.exploration.questions[0].clone();
    let request = flow
        .exploration
        .request(
            Some(AnswerInput {
                option: Some("keep".into()),
                ..Default::default()
            }),
            Some(&question),
        )
        .unwrap();
    flow.fixture
        .commands
        .send(WorkerCommand::Explore(ExploreCommand::Turn(Box::new(
            request.clone(),
        ))))
        .unwrap();
    flow.wait_for_prompt(&request);
    let payload = serde_json::json!({"instance":request.instance,"request":request.request,"checkpoint":request.checkpoint,
        "interpretation":{"answer":request.answer.unwrap().id,"status":"accepted","recap":"Keep policy","follow_ups":[]},
        "summary":"Check Files next", "to_be_implemented":"Only agreed work", "future_work":"Later"});
    let result = flow.call_with_ack("submit_conclusion", payload.clone(), false);
    assert_eq!(result.is_error, Some(true));
    let recorded = flow.saved().exploration.conversation;
    assert_eq!(recorded.len(), 2);
    let before = fs::read(flow.fixture.herdr.directory.path().join("prompt.txt")).unwrap();
    let restored = flow.reopen().result.unwrap().unwrap();
    assert_eq!(restored.exploration.conversation, recorded);
    assert_eq!(
        fs::read(flow.fixture.herdr.directory.path().join("prompt.txt")).unwrap(),
        before
    );
    flow.exploration = restored.exploration.clone();
    let follow_up = flow
        .exploration
        .request(
            Some(AnswerInput {
                text: "Explain the agreed scope".into(),
                ..Default::default()
            }),
            None,
        )
        .unwrap();
    flow.fixture
        .commands
        .send(WorkerCommand::Explore(ExploreCommand::Turn(Box::new(
            follow_up.clone(),
        ))))
        .unwrap();
    flow.wait_for_prompt(&follow_up);
    let result = flow.call("submit_conclusion", payload.clone());
    assert!(
        result.content[0]
            .as_text()
            .unwrap()
            .text
            .contains("\"applied\":false")
    );
    let mut conflicting = payload;
    conflicting["summary"] = "Rewritten history".into();
    assert_eq!(
        flow.call("submit_conclusion", conflicting).is_error,
        Some(true)
    );
    let pass = flow.saved();
    assert_eq!(pass.exploration.conversation, recorded);
    assert_eq!(pass.exploration.pending_request(), Some(&follow_up));
    flow.finish();
}

#[test]
fn cancellation_from_an_old_attempt_cannot_fail_the_retried_logical_turn() {
    let mut flow = ExploreFlow::start(RepoType::Git);
    flow.turn(None, 1);
    let question = flow.exploration.questions[0].clone();
    let request = flow
        .exploration
        .request(
            Some(AnswerInput {
                option: Some("keep".into()),
                ..Default::default()
            }),
            Some(&question),
        )
        .unwrap();
    flow.fixture
        .commands
        .send(WorkerCommand::Explore(ExploreCommand::Turn(Box::new(
            request.clone(),
        ))))
        .unwrap();
    flow.wait_for_prompt(&request);
    let old_attempt = flow.saved().turns[&request.request].attempt.clone();
    flow.fixture
        .commands
        .send(WorkerCommand::Explore(ExploreCommand::Cancel))
        .unwrap();
    flow.exploration.cancel();
    let retry = flow.exploration.retry().unwrap();
    flow.fixture
        .commands
        .send(WorkerCommand::Explore(ExploreCommand::Turn(Box::new(
            retry.clone(),
        ))))
        .unwrap();
    flow.wait_for_prompt(&retry);
    flow.fixture
        .commands
        .send(WorkerCommand::ExploreFinished {
            attempt: old_attempt,
            event: Box::new(ui_events::ExploreFinished {
                instance: request.instance.clone(),
                request: request.request.clone(),
                result: Err("Late cancellation".into()),
            }),
        })
        .unwrap();
    // A real MCP roundtrip runs after the late receipt and must still be able to finish this turn.
    let payload = serde_json::json!({"instance":request.instance,"request":request.request,"checkpoint":request.checkpoint,
        "interpretation":{"answer":request.answer.unwrap().id,"status":"accepted","recap":"Keep policy","follow_ups":[]},
        "summary":"Check Files next", "to_be_implemented":"", "future_work":""});
    assert_ne!(flow.call("submit_conclusion", payload).is_error, Some(true));
    assert_eq!(flow.saved().exploration.answers.len(), 1);
    flow.finish();
}

#[test]
fn restored_history_blocks_a_different_conversation_and_continues_when_original_returns() {
    let mut flow = ExploreFlow::start(RepoType::Git);
    flow.fixture.herdr.report_session("original-conversation");
    flow.turn(None, 1);
    flow.exploration = flow.reopen().result.unwrap().unwrap().exploration.clone();
    flow.fixture.herdr.stop_agent();
    flow.fixture.herdr.start_agent();
    flow.fixture.herdr.report_session("different-conversation");
    let question = flow.exploration.questions[0].clone();
    let request = flow
        .exploration
        .request(
            Some(AnswerInput {
                option: Some("keep".into()),
                text: "Exact posted context".into(),
                ..Default::default()
            }),
            Some(&question),
        )
        .unwrap();
    flow.fixture
        .commands
        .send(WorkerCommand::Explore(ExploreCommand::Turn(Box::new(
            request.clone(),
        ))))
        .unwrap();
    loop {
        let event = flow
            .fixture
            .messages
            .recv_timeout(Duration::from_secs(10))
            .unwrap();
        if let Some(event) = event.downcast_ref::<ui_events::ExploreFinished>() {
            assert!(
                event
                    .result
                    .as_ref()
                    .unwrap_err()
                    .contains("Different agent conversation")
            );
            break;
        }
    }
    assert_eq!(
        fs::read(flow.fixture.herdr.directory.path().join("prompt.txt"))
            .unwrap()
            .len(),
        flow.prompt_offset
    );
    flow.fixture.herdr.stop_agent();
    flow.fixture.herdr.start_agent();
    flow.fixture.herdr.report_session("original-conversation");
    flow.exploration = flow.reopen().result.unwrap().unwrap().exploration.clone();
    flow.exploration.pause_delivery();
    let retry = flow.exploration.retry().unwrap();
    assert_eq!(retry.answer, request.answer);
    flow.fixture
        .commands
        .send(WorkerCommand::Explore(ExploreCommand::Turn(Box::new(
            retry.clone(),
        ))))
        .unwrap();
    flow.wait_for_prompt(&retry);
    assert_eq!(flow.saved().exploration.answers.len(), 1);
    flow.finish();
}

#[test]
fn explicit_retry_adopts_changed_native_session_in_the_same_pane() {
    let mut flow = ExploreFlow::start(RepoType::Git);
    flow.fixture.herdr.report_session("original-conversation");
    flow.turn(None, 1);
    let question = flow.exploration.questions[0].clone();
    let request = flow
        .exploration
        .request(
            Some(AnswerInput {
                option: Some("keep".into()),
                text: "Keep the existing policy".into(),
                ..Default::default()
            }),
            Some(&question),
        )
        .unwrap();
    flow.fixture.herdr.stop_agent();
    flow.fixture.herdr.start_agent();
    flow.fixture.herdr.report_session("refreshed-conversation");
    flow.fixture
        .commands
        .send(WorkerCommand::Explore(ExploreCommand::Turn(Box::new(
            request.clone(),
        ))))
        .unwrap();
    loop {
        let event = flow
            .fixture
            .messages
            .recv_timeout(Duration::from_secs(10))
            .unwrap();
        if let Some(event) = event.downcast_ref::<ui_events::ExploreFinished>() {
            assert!(
                event
                    .result
                    .as_ref()
                    .unwrap_err()
                    .contains("different agent conversation")
            );
            break;
        }
    }
    assert!(flow.saved().binding.as_ref().is_some_and(|binding| {
        !binding.matches(
            &flow
                .fixture
                .herdr
                .client()
                .get_agent(&flow.fixture.herdr.pane_id)
                .unwrap()
                .unwrap(),
        )
    }));
    flow.exploration = flow.reopen().result.unwrap().unwrap().exploration.clone();
    flow.exploration.pause_delivery();
    let retry = flow.exploration.retry().unwrap();
    assert_eq!(retry.answer, request.answer);
    flow.fixture
        .commands
        .send(WorkerCommand::Explore(ExploreCommand::Retry(Box::new(
            retry.clone(),
        ))))
        .unwrap();
    flow.wait_for_prompt(&retry);
    let current = flow
        .fixture
        .herdr
        .client()
        .get_agent(&flow.fixture.herdr.pane_id)
        .unwrap()
        .unwrap();
    assert!(flow.saved().binding.unwrap().matches(&current));
    let response = serde_json::json!({
        "instance":retry.instance,"request":retry.request,"checkpoint":retry.checkpoint,
        "interpretation":{"answer":retry.answer.unwrap().id,"status":"accepted","recap":"Keep the policy","follow_ups":[]},
        "reply":{"text":"The saved policy still applies.","evidence":[]},
        "topics":[{"id":"topic2","title":"Remaining policy","entries":[{"path":"reviewed.rs","side":"new","lines":null}],"status":"open"}],
        "next":{"id":"q2","version":1,"topic":"topic2","text":"Keep the remaining policy?",
            "alternatives":[{"id":"keep","text":"Keep it","outcome":"accepted"},{"id":"change","text":"Change it","outcome":"needs_follow_up"}],
            "evidence":[{"path":"reviewed.rs","side":"new","lines":{"first_line":1,"last_line":1},"notes":"Shows the remaining policy"}]},
        "limitations":[],"findings":[]
    });
    let accepted = flow.submit(&response);
    assert_ne!(accepted.is_error, Some(true), "{accepted:?}");
    flow.finish();
}

#[test]
fn unresolved_original_identity_restores_history_without_guessing_a_binding() {
    let mut flow = ExploreFlow::start(RepoType::Git);
    flow.turn(None, 1);
    assert!(flow.saved().binding.is_none());
    flow.exploration = flow.reopen().result.unwrap().unwrap().exploration.clone();
    flow.fixture
        .herdr
        .report_session("cannot-prove-this-is-original");
    let question = flow.exploration.questions[0].clone();
    let request = flow
        .exploration
        .request(
            Some(AnswerInput {
                option: Some("keep".into()),
                ..Default::default()
            }),
            Some(&question),
        )
        .unwrap();
    flow.fixture
        .commands
        .send(WorkerCommand::Explore(ExploreCommand::Turn(Box::new(
            request,
        ))))
        .unwrap();
    loop {
        let event = flow
            .fixture
            .messages
            .recv_timeout(Duration::from_secs(10))
            .unwrap();
        if let Some(event) = event.downcast_ref::<ui_events::ExploreFinished>() {
            assert!(
                event
                    .result
                    .as_ref()
                    .unwrap_err()
                    .contains("binding was never established")
            );
            break;
        }
    }
    assert_eq!(
        fs::read(flow.fixture.herdr.directory.path().join("prompt.txt"))
            .unwrap()
            .len(),
        flow.prompt_offset
    );
    assert_eq!(flow.saved().exploration.questions.len(), 1);
    flow.finish();
}
