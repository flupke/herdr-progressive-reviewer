use super::*;
use review_explore::{AnswerInput, Command as ExploreCommand, Exploration, TopicStatus};

#[path = "explore/conclusion.tests.rs"]
mod conclusion;

impl ReviewFlowFixture {
    fn explore(&self, command: ExploreCommand) {
        self.runtime.perform([Action::Explore(command)]);
    }
}

struct ExploreFlow {
    fixture: ReviewFlowFixture,
    exploration: Exploration,
    prompt_offset: usize,
    endpoint: review_mcp::Endpoint,
    access: String,
}

impl ExploreFlow {
    fn start(kind: RepoType) -> Self {
        let mut fixture = ReviewFlowFixture::start(kind);
        fixture.explore(ExploreCommand::Start);
        let comparison = loop {
            let event = fixture
                .runtime
                .recv_timeout(crate::runtime::tests::HERDR_WAIT)
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
            .explore(ExploreCommand::Turn(Box::new(request.clone())));
        self.wait_for_prompt(&request);
        let interpretation = request.answer.as_ref().map(|answer| serde_json::json!({
            "answer":answer.id,"status":"needs_follow_up","recap":"Recorded: keep resolved; add regression test — follow-up.","follow_ups":["Add regression test"]
        }));
        let response = serde_json::json!({
            "instance":request.instance,"request":request.request,"checkpoint":request.checkpoint,
            "interpretation":interpretation, "reply":{"text":"I checked the policy.","evidence":[]},
            "topics":[{"id":format!("topic{version}"),"title":"Policy consequence","entries":[{"path":"reviewed.rs","side":"new","lines":null}],"status":"open"}],
            "next":{"id":format!("q{version}"),"version":1,"topic":format!("topic{version}"),"text":"Keep resolved conversations resolved?",
                "rationale":null,"visual":null,"alternatives":[{"id":"keep","text":"Keep resolved","outcome":"accepted"},{"id":"change","text":"Reopen","outcome":"needs_follow_up"}],"evidence":[{"path":"reviewed.rs","side":"new","lines":{"first_line":1,"last_line":1},"notes":"Implements the policy that determines whether completed conversations should reopen"}]},
            "design":request.is_kickoff().then(|| serde_json::json!({
                "overview":"A policy.","data_flow":"None.","algorithm":"None.","alternatives":"None."
            })),
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
        let deadline = Instant::now() + crate::runtime::tests::HERDR_WAIT;
        let response = loop {
            if let Ok(result) = result.try_recv() {
                break result;
            }
            assert!(Instant::now() < deadline, "MCP response timed out");
            if let Some(event) = self.fixture.runtime.recv_timeout(Duration::from_millis(20))
                && let Some(event) = event.downcast_ref::<ui_events::ExploreCommitted>()
                && acknowledge
            {
                self.exploration = event.round.exploration.clone();
                event.response.send(Ok(event.applied)).unwrap();
            }
        };
        client.join().unwrap();
        response
    }

    fn wait_for_prompt(&mut self, request: &review_explore::TurnRequest) {
        let deadline = Instant::now() + crate::runtime::tests::HERDR_WAIT;
        loop {
            let text = fs::read_to_string(self.fixture.herdr.server.root().join("prompt.txt"))
                .unwrap_or_default();
            if let Some(prompt) = text.get(self.prompt_offset..)
                && let Some(access) = prompt
                    .lines()
                    .find_map(|line| line.strip_prefix("Explore review access: "))
            {
                assert!(prompt.contains(&format!("Explore request: {}\n", request.request)));
                assert!(prompt.contains(&format!("Explore round: {}\n", request.instance)));
                assert!(
                    prompt.contains(&format!("Checkpoint: {}\n", request.checkpoint.checkpoint))
                );
                assert!(prompt.contains(&format!(
                    "Review unit: {}\n",
                    request.checkpoint.review_unit.as_str()
                )));
                self.access = access.to_owned();
                if let Some(answer) = &request.answer {
                    assert!(prompt.contains(&format!("Answer ID: {}\n", answer.id)));
                    assert!(!prompt.contains("Repository root:"));
                } else {
                    assert!(prompt.contains(&request.checkpoint.checkpoint));
                    assert!(
                        prompt.contains(self.fixture.runtime.repository.root().to_str().unwrap())
                    );
                }
                self.prompt_offset = text.len();
                return;
            }
            assert!(
                Instant::now() < deadline,
                "Explore prompt was not delivered: {:?}",
                self.fixture
                    .runtime
                    .drain_events()
                    .iter()
                    .filter_map(|event| event.downcast_ref::<ui_events::ExploreFinished>().cloned())
                    .collect::<Vec<_>>()
            );
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn saved(&self) -> review_explore::ExploreRound {
        ReviewStore::open(
            self.fixture.runtime.state.path(),
            self.fixture.runtime.repository.root(),
        )
        .unwrap()
        .load_explore(&self.fixture.review_unit, &self.exploration.instance)
        .unwrap()
        .unwrap()
    }

    fn native_status(&self, status: herdr_client::protocol::AgentStatus) {
        self.fixture.herdr.release_agent();
        fs::write(
            self.fixture.herdr.server.root().join("prompt.state"),
            if status == herdr_client::protocol::AgentStatus::Working {
                "⠋ Working"
            } else {
                "✳ Ready"
            },
        )
        .unwrap();
        let deadline = Instant::now() + crate::runtime::tests::HERDR_WAIT;
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

    fn finish(self) {
        drop(self.fixture);
    }

    /// Wait until the session reports that the prompt of `request` failed.
    fn wait_for_failure(&mut self, request: &review_explore::TurnRequest) {
        let deadline = Instant::now() + crate::runtime::tests::HERDR_WAIT;
        loop {
            assert!(
                Instant::now() < deadline,
                "the turn did not fail: {:?}",
                self.saved().turns[&request.request].state
            );
            if let Some(event) = self.fixture.runtime.recv_timeout(Duration::from_millis(20))
                && let Some(finished) = event.downcast_ref::<ui_events::ExploreFinished>()
                && finished.request == request.request
            {
                assert!(finished.result.is_err(), "{:?}", finished.result);
                return;
            }
        }
    }

    /// Wait until the saved round records `state` for the latest prompt of `request`.
    fn wait_for_dispatch(
        &self,
        request: &review_explore::TurnRequest,
        state: &review_explore::DispatchState,
    ) {
        let deadline = Instant::now() + crate::runtime::tests::HERDR_WAIT;
        loop {
            let saved = self.saved().turns[&request.request].state.clone();
            if saved == *state {
                return;
            }
            assert!(Instant::now() < deadline, "the turn stayed {saved:?}");
            thread::sleep(Duration::from_millis(20));
        }
    }

    /// Forget the prompts the agent already read, so that waiting for a prompt waits for a new
    /// one.
    fn skip_prompts(&mut self) {
        self.prompt_offset =
            fs::read_to_string(self.fixture.herdr.server.root().join("prompt.txt"))
                .unwrap_or_default()
                .len();
    }

    fn enqueue(&mut self) -> review_explore::TurnRequest {
        let request = self.exploration.request(None, None).unwrap();
        self.fixture
            .explore(ExploreCommand::Turn(Box::new(request.clone())));
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
    // An agent that is working already counts as started, even with no new activity.
    flow.fixture.herdr.swallow_prompts(true);
    flow.native_status(herdr_client::protocol::AgentStatus::Working);
    let request = flow.enqueue();
    flow.wait_for_prompt(&request);
    flow.wait_for_dispatch(&request, &review_explore::DispatchState::Delivered);
    thread::sleep(Duration::from_millis(350));
    let text = fs::read_to_string(flow.fixture.herdr.server.root().join("prompt.txt")).unwrap();
    assert_eq!(text.matches("Explore request: ").count(), 1);
    flow.finish();
}

#[test]
fn a_kickoff_the_agent_does_not_start_on_waits_for_a_retry_of_the_same_request() {
    let mut flow = ExploreFlow::start(RepoType::Git);
    flow.fixture.herdr.swallow_prompts(true);
    let request = flow.exploration.request(None, None).unwrap();
    flow.fixture
        .explore(ExploreCommand::Turn(Box::new(request.clone())));

    flow.wait_for_failure(&request);
    flow.wait_for_dispatch(&request, &review_explore::DispatchState::NotStarted);

    flow.skip_prompts();
    flow.fixture.herdr.swallow_prompts(false);
    flow.fixture
        .explore(ExploreCommand::Retry(Box::new(request.clone())));
    flow.wait_for_prompt(&request);
    flow.wait_for_dispatch(&request, &review_explore::DispatchState::Delivered);
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
        .runtime
        .files
        .write("unchanged-caller.rs", b"external edit\n");
    flow.fixture.runtime.effects.refresh().unwrap();
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
fn late_session_detection_preserves_the_interview_and_a_new_send_selects_the_replacement() {
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
    flow.fixture.herdr.show_idle();
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
        .explore(ExploreCommand::Turn(Box::new(request.clone())));
    flow.wait_for_prompt(&request);
    assert!(
        flow.saved().last_agent_session.unwrap().matches(
            &flow
                .fixture
                .herdr
                .client()
                .get_agent(&flow.fixture.herdr.pane_id)
                .unwrap()
                .unwrap()
        )
    );
    flow.finish();
}

#[test]
fn cancelled_explore_access_rejects_submissions_without_changing_history() {
    let mut flow = ExploreFlow::start(RepoType::Git);
    flow.turn(None, 1);
    let previous = serde_json::to_value(&flow.exploration.conversation[0].update).unwrap();
    flow.fixture.explore(ExploreCommand::Cancel);
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
