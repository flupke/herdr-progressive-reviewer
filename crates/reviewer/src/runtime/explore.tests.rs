use super::*;
use review_explore::{AnswerInput, Command as ExploreCommand, Exploration, TopicStatus};

#[path = "explore/conclusion.tests.rs"]
mod conclusion;
#[path = "explore/recovery.tests.rs"]
mod recovery;

struct ExploreFlow {
    fixture: GuideFlowFixture,
    exploration: Exploration,
    prompt_offset: usize,
    endpoint: review_mcp::Endpoint,
    access: String,
}

impl ExploreFlow {
    fn start(kind: RepoType) -> Self {
        let fixture = GuideFlowFixture::start(kind);
        fixture
            .commands
            .send(WorkerCommand::Explore(ExploreCommand::Start))
            .unwrap();
        let comparison = loop {
            let event = fixture
                .messages
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
            if let Some(event) = event.downcast_ref::<ui_events::ExploreCaptured>() {
                break event.result.clone().unwrap();
            }
        };
        let endpoint = fixture.endpoint;
        Self {
            endpoint,
            access: String::new(),
            fixture,
            exploration: Exploration::new(comparison),
            prompt_offset: 0,
        }
    }

    fn turn(&mut self, answer: Option<AnswerInput>, version: u32) {
        let question = self.exploration.questions.last().cloned();
        let request = self.exploration.request(answer, question.as_ref()).unwrap();
        self.fixture
            .commands
            .send(WorkerCommand::Explore(ExploreCommand::Turn(Box::new(
                request.clone(),
            ))))
            .unwrap();
        self.wait_for_prompt(&request);
        let interpretation = request.answer.as_ref().map(|answer| serde_json::json!({
            "answer":answer.id,"status":"needs_follow_up","recap":"Recorded: keep resolved; add regression test — follow-up.","follow_ups":["Add regression test"]
        }));
        let response = serde_json::json!({
            "instance":request.instance,"request":request.request,"checkpoint":request.checkpoint,
            "interpretation":interpretation, "reply":{"text":"I checked the policy.","evidence":[]},
            "topics":[{"id":format!("topic{version}"),"title":"Policy consequence","entries":[{"path":"reviewed.rs","side":"new","lines":null}],"status":"open"}],
            "next":{"id":format!("q{version}"),"version":1,"topic":format!("topic{version}"),"text":"Keep resolved conversations resolved?",
                "rationale":null,"visual":null,"alternatives":[{"id":"keep","text":"Keep resolved","outcome":"accepted"},{"id":"change","text":"Reopen","outcome":"needs_follow_up"}],"evidence":[{"path":"reviewed.rs","side":"new","lines":{"first_line":1,"last_line":1},"relationship":"Implements policy","decision_relevance":"This determines whether completed conversations should reopen"}]},
            "conclusion":null,"limitations":[],"findings":[]
        });
        for field in ["instance", "request"] {
            let mut mismatched = response.clone();
            mismatched[field] = "another-turn".into();
            let rejected = self.submit(&mismatched);
            assert_eq!(rejected.is_error, Some(true), "{rejected:?}");
        }
        if version == 1 {
            let mut invalid = response.clone();
            invalid["next"]["alternatives"] = serde_json::json!([]);
            let rejected = self.submit(&invalid);
            assert_eq!(rejected.is_error, Some(true));
            assert!(
                serde_json::to_string(&rejected)
                    .unwrap()
                    .contains("two to five"),
                "{rejected:?}"
            );
            assert!(self.exploration.questions.is_empty());
        }
        let accepted = self.submit(&response);
        assert_ne!(accepted.is_error, Some(true), "{accepted:?}");
        let retry = self.submit(&response);
        assert_ne!(retry.is_error, Some(true), "{retry:?}");
        assert!(
            retry.content[0]
                .as_text()
                .unwrap()
                .text
                .contains("\"applied\":false")
        );
    }

    fn submit(&mut self, update: &serde_json::Value) -> rmcp::model::CallToolResult {
        self.call(
            "submit_question",
            serde_json::json!({"review":update["instance"],"update":update}),
        )
    }

    fn call(
        &mut self,
        tool: &'static str,
        arguments: serde_json::Value,
    ) -> rmcp::model::CallToolResult {
        self.call_with_ack(tool, arguments, true)
    }

    fn call_with_ack(
        &mut self,
        tool: &'static str,
        mut arguments: serde_json::Value,
        acknowledge: bool,
    ) -> rmcp::model::CallToolResult {
        arguments["review"] = self.access.clone().into();
        let endpoint = self.endpoint;
        let (sent, result) = mpsc::channel();
        let client = thread::spawn(move || {
            use rmcp::{
                ServiceExt,
                model::{CallToolRequestParams, ClientInfo},
                transport::StreamableHttpClientTransport,
            };
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    let client = ClientInfo::default()
                        .serve(StreamableHttpClientTransport::from_uri(endpoint.url()))
                        .await
                        .unwrap();
                    let result = client
                        .call_tool(
                            CallToolRequestParams::new(tool)
                                .with_arguments(arguments.as_object().unwrap().clone()),
                        )
                        .await
                        .unwrap();
                    client.cancel().await.unwrap();
                    sent.send(result).unwrap();
                });
        });
        let deadline = Instant::now() + Duration::from_secs(20);
        let response = loop {
            if let Ok(result) = result.try_recv() {
                break result;
            }
            assert!(Instant::now() < deadline, "MCP response timed out");
            if let Ok(event) = self
                .fixture
                .messages
                .recv_timeout(Duration::from_millis(20))
                && let Some(event) = event.downcast_ref::<ui_events::ExploreCommitted>()
                && acknowledge
            {
                self.exploration = event.pass.exploration.clone();
                event.response.send(Ok(event.applied)).unwrap();
            }
        };
        client.join().unwrap();
        response
    }

    fn wait_for_prompt(&mut self, request: &review_explore::TurnRequest) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let text = fs::read_to_string(self.fixture.herdr.directory.path().join("prompt.txt"))
                .unwrap_or_default();
            if let Some(prompt) = text.get(self.prompt_offset..)
                && let Some(access) = prompt
                    .lines()
                    .find_map(|line| line.strip_prefix("Explore review access: "))
            {
                assert!(prompt.contains(&format!("Explore request: {}\n", request.request)));
                assert!(prompt.contains(&format!("Explore pass: {}\n", request.instance)));
                assert!(
                    prompt.contains(&format!("Checkpoint: {}\n", request.checkpoint.checkpoint))
                );
                assert!(prompt.contains(&format!(
                    "Review unit: {}\n",
                    request.checkpoint.review_unit.as_str()
                )));
                assert!(!prompt.contains("Mailbox:"));
                assert!(!prompt.contains("get_explore"));
                assert!(!prompt.contains("Turn input (JSON)"));
                self.access = access.to_owned();
                if let Some(answer) = &request.answer {
                    assert!(prompt.contains(&format!("Answer ID: {}\n", answer.id)));
                    assert!(!prompt.contains("Conduct one turn of experimental Explore"));
                    assert!(prompt.contains("submit_question"));
                } else {
                    assert!(prompt.contains("Submit the first question directly"));
                    assert!(prompt.contains(&request.checkpoint.checkpoint));
                    assert!(prompt.contains(self.fixture.repository.root().to_str().unwrap()));
                }
                self.prompt_offset = text.len();
                return;
            }
            assert!(
                Instant::now() < deadline,
                "Explore prompt was not delivered: {:?}",
                self.fixture
                    .messages
                    .try_iter()
                    .filter_map(|event| event.downcast_ref::<ui_events::ExploreFinished>().cloned())
                    .collect::<Vec<_>>()
            );
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn finish(self) {
        self.fixture.commands.send(WorkerCommand::Quit).unwrap();
        self.fixture.worker_thread.join().unwrap();
    }

    fn enqueue(&mut self) -> review_explore::TurnRequest {
        let request = self.exploration.request(None, None).unwrap();
        self.fixture
            .commands
            .send(WorkerCommand::Explore(ExploreCommand::Turn(Box::new(
                request.clone(),
            ))))
            .unwrap();
        // A rejected submission confirms preparation reached the serial runtime owner.
        let result = self.submit(&Self::empty_update(&request));
        assert_eq!(result.is_error, Some(true));
        assert!(
            result.content[0]
                .as_text()
                .unwrap()
                .text
                .contains("Obsolete Explore access")
        );
        request
    }

    fn empty_update(request: &review_explore::TurnRequest) -> serde_json::Value {
        serde_json::json!({
            "instance":request.instance,"request":request.request,"checkpoint":request.checkpoint,
            "interpretation":null,"reply":null,"topics":[],"next":null,"conclusion":null,
            "limitations":[],"findings":[]
        })
    }
}

#[test]
fn explore_turn_prompts_a_working_agent_once() {
    let mut flow = ExploreFlow::start(RepoType::Git);
    flow.native_status(herdr_client::protocol::AgentStatus::Working);
    let request = flow.enqueue();
    flow.wait_for_prompt(&request);
    thread::sleep(Duration::from_millis(350));
    let text = fs::read_to_string(flow.fixture.herdr.directory.path().join("prompt.txt")).unwrap();
    assert_eq!(text.matches("Explore request: ").count(), 1);
    flow.finish();
}

#[test_case::test_case(RepoType::Git; "git")]
#[test_case::test_case(RepoType::Jj; "jj")]
fn selected_agent_receives_working_copy_turns_without_freshness_checks(kind: RepoType) {
    let mut flow = ExploreFlow::start(kind);
    flow.turn(None, 1);
    flow.turn(
        Some(AnswerInput {
            option: Some("keep".into()),
            text: "Keep resolved only if a regression test is added.\nPreserve \"{{TURN}}\" exactly — thanks.".into(),
            ..AnswerInput::default()
        }),
        2,
    );
    assert_eq!(flow.exploration.answers.len(), 1);
    assert_eq!(
        flow.exploration.topics["topic1"].status,
        TopicStatus::NeedsFollowUp
    );
    flow.fixture
        .repository_files
        .write("unchanged-caller.rs", b"external edit\n");
    flow.fixture.commands.send(WorkerCommand::Poll).unwrap();
    // This milestone assumes code stays unchanged; edits do not block another turn.
    flow.turn(
        Some(AnswerInput {
            text: "Continue with the working copy".into(),
            ..AnswerInput::default()
        }),
        3,
    );
    flow.finish();
}

#[test]
fn late_session_detection_preserves_the_interview_but_a_replacement_does_not_receive_answers() {
    let mut flow = ExploreFlow::start(RepoType::Git);
    flow.turn(None, 1);
    flow.fixture.herdr.report_session("late-native-session");
    flow.turn(
        Some(AnswerInput {
            text: "Keep the policy from the first question.".into(),
            ..AnswerInput::default()
        }),
        2,
    );
    assert_eq!(flow.exploration.answers.len(), 1);
    flow.fixture.herdr.stop_agent();
    flow.fixture.herdr.start_agent();
    flow.fixture.herdr.wait_for_agent(None);
    flow.fixture.herdr.report_agent("idle");
    flow.fixture
        .herdr
        .report_session("different-native-session");
    let previous =
        serde_json::to_value(&flow.exploration.conversation.last().unwrap().update).unwrap();
    let rejected = flow.submit(&previous);
    assert_eq!(rejected.is_error, Some(true));
    assert!(
        serde_json::to_string(&rejected)
            .unwrap()
            .contains("different agent conversation")
    );
    let question = flow.exploration.questions.last().cloned().unwrap();
    let request = flow
        .exploration
        .request(
            Some(AnswerInput {
                text: "Do not deliver this answer to another conversation.".into(),
                ..AnswerInput::default()
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
    let error = loop {
        let event = flow
            .fixture
            .messages
            .recv_timeout(Duration::from_secs(10))
            .unwrap();
        if let Some(finished) = event.downcast_ref::<ui_events::ExploreFinished>() {
            assert_eq!(finished.request, request.request);
            break finished.result.clone().unwrap_err();
        }
    };
    assert!(error.contains("different agent conversation"), "{error}");
    assert_eq!(
        fs::read_to_string(flow.fixture.herdr.directory.path().join("prompt.txt"))
            .unwrap()
            .len(),
        flow.prompt_offset
    );
    assert!(flow.exploration.failed(&request.request, &error));
    assert_eq!(flow.exploration.retry().unwrap().answer, request.answer);
    flow.finish();
}

#[test]
fn cancelled_explore_access_rejects_submissions_without_changing_history() {
    let mut flow = ExploreFlow::start(RepoType::Git);
    flow.turn(None, 1);
    let previous = serde_json::to_value(&flow.exploration.conversation[0].update).unwrap();
    flow.fixture
        .commands
        .send(WorkerCommand::Explore(ExploreCommand::Cancel))
        .unwrap();
    let rejected = flow.submit(&previous);
    assert_eq!(rejected.is_error, Some(true));
    assert!(
        serde_json::to_string(&rejected)
            .unwrap()
            .contains("Obsolete Explore access")
    );
    assert_eq!(flow.exploration.conversation.len(), 1);
    flow.finish();
}
