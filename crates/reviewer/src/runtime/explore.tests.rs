use super::*;
use review_explore::{AnswerInput, Command as ExploreCommand, Exploration, TopicStatus};

#[path = "explore/conclusion.tests.rs"]
mod conclusion;

impl ReviewFlowFixture {
    pub(super) fn explore(&self, command: ExploreCommand) {
        self.runtime.perform([Action::Explore(command)]);
    }
}

pub(super) struct ExploreFlow {
    pub(super) fixture: ReviewFlowFixture,
    pub(super) exploration: Exploration,
    /// How many of the agent's prompts the test went through.
    prompt_offset: usize,
    endpoint: review_mcp::Endpoint,
    pub(super) access: String,
    /// What the reviewer saved.
    pub(super) state: SavedState,
    /// Whether the agent holds each turn until the test submitted it, as an agent does that
    /// calls the reviewer's tools during its turn.
    pub(super) holds_turns: bool,
}

/// Says that an MCP call of the test has its answer.
struct McpAnswered;

impl ExploreFlow {
    fn start(kind: RepoType) -> Self {
        Self::start_on(ReviewFlowFixture::start(kind, |_| {}))
    }

    /// Starts a round in `fixture`, and waits for its change to be captured.
    pub(super) fn start_on(mut fixture: ReviewFlowFixture) -> Self {
        let state = SavedState::watch(fixture.runtime.state.path());
        fixture.explore(ExploreCommand::Start);
        let comparison = fixture
            .runtime
            .wait_for::<ui_events::ExploreCaptured>()
            .result
            .unwrap();
        let endpoint = fixture.endpoint;
        Self {
            endpoint,
            access: String::new(),
            fixture,
            exploration: Exploration::new(comparison),
            prompt_offset: 0,
            state,
            holds_turns: false,
        }
    }

    /// Posts the turn `request` as the pane does, and adopts the round saved, as the pane does
    /// too: the session may save an answer under other identities. Returns the turn as saved.
    pub(super) fn post_turn(
        &mut self,
        request: &review_explore::TurnRequest,
    ) -> review_explore::TurnRequest {
        self.fixture
            .explore(ExploreCommand::Turn(Box::new(request.clone())));
        loop {
            let event = self.fixture.runtime.next_event();
            if let Some(posted) = event.downcast_ref::<ui_events::ExplorePosted>()
                && posted.request.request == request.request
            {
                let round = posted.result.as_ref().expect("the turn is saved");
                self.exploration = round.exploration.clone();
                return round.exploration.retry_request().unwrap().clone();
            }
        }
    }

    pub(super) fn turn(&mut self, answer: Option<AnswerInput>, version: u32) {
        let question = self.exploration.questions.last().cloned();
        let request = self.exploration.request(answer, question.as_ref()).unwrap();
        let from = self.fixture.herdr.events().received().len();
        let request = self.post_turn(&request);
        self.wait_for_prompt(&request);
        let statuses = self.holds_turns.then(|| {
            // The agent submits during its turn, which Herdr saw start.
            self.fixture.herdr.wait_for_agent_event(
                from,
                "the agent's turn",
                &StandInEvent::TurnStarted,
            );
            let statuses = self
                .fixture
                .herdr
                .server
                .agent_statuses(&self.fixture.herdr.pane_id);
            statuses.wait_for(herdr_client::protocol::AgentStatus::Working);
            statuses
        });
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
                "thesis":"A policy.","overview":{"thesis":"A policy.", "body":"A policy."},"data_flow":{"thesis":"None.", "body":"None."},"algorithm":{"thesis":"None.", "body":"None."},"alternatives":{"thesis":"None.", "body":"None."}
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
        if let Some(statuses) = statuses {
            self.fixture
                .herdr
                .events()
                .send(&StandInRole::Agent, &StandInCommand::EndTurn);
            statuses.wait_for(herdr_client::protocol::AgentStatus::Idle);
        }
    }

    pub(super) fn submit(&mut self, update: &serde_json::Value) -> rmcp::model::CallToolResult {
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
        let answered = self.fixture.runtime.background.clone();
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
                    answered
                        .send(component_core::EventEnvelope::new(McpAnswered))
                        .unwrap();
                });
        });
        loop {
            let event = self.fixture.runtime.next_event();
            if event.downcast_ref::<McpAnswered>().is_some() {
                break;
            }
            if let Some(event) = event.downcast_ref::<ui_events::ExploreCommitted>()
                && acknowledge
            {
                self.exploration = event.round.exploration.clone();
                // A round committed with no submit waiting, such as a turn run-ahead prepared,
                // has nobody to hear the acknowledgement.
                let _ = event.response.send(Ok(event.applied));
            }
        }
        client.join().unwrap();
        result.recv().unwrap()
    }

    /// Waits for the prompt of `request`, the first prompt with an access value among those
    /// the agent read after the ones the test went through, and goes through them.
    pub(super) fn wait_for_prompt(&mut self, request: &review_explore::TurnRequest) {
        let offset = self.prompt_offset;
        let has_access = |prompts: &[String]| {
            prompts.get(offset..).is_some_and(|new| {
                new.iter()
                    .any(|prompt| prompt.contains("Explore review access: "))
            })
        };
        let prompts = self
            .fixture
            .herdr
            .wait_for_prompts("the Explore prompt", has_access);
        let prompt = prompts_text(&prompts[offset..]);
        let access = prompt
            .lines()
            .find_map(|line| line.strip_prefix("Explore review access: "))
            .unwrap();
        assert!(prompt.contains(&format!("Explore request: {}\n", request.request)));
        assert!(prompt.contains(&format!("Explore round: {}\n", request.instance)));
        assert!(prompt.contains(&format!("Checkpoint: {}\n", request.checkpoint.checkpoint)));
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
            assert!(prompt.contains(self.fixture.runtime.repository.root().to_str().unwrap()));
        }
        self.prompt_offset = prompts.len();
    }

    pub(super) fn saved(&self) -> review_explore::ExploreRound {
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
        if status == herdr_client::protocol::AgentStatus::Working {
            self.fixture.herdr.show_working();
        } else {
            self.fixture.herdr.show_idle();
        }
    }

    fn finish(self) {
        drop(self.fixture);
    }

    /// Wait until the session reports that the prompt of `request` failed.
    fn wait_for_failure(&mut self, request: &review_explore::TurnRequest) {
        loop {
            let event = self.fixture.runtime.next_event();
            if let Some(finished) = event.downcast_ref::<ui_events::ExploreFinished>()
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
        let saved = || self.saved().turns[&request.request].state.clone();
        self.state.wait_until(
            "the turn's dispatch",
            || (saved() == *state).then_some(()),
            || format!("the turn stayed {:?}", saved()),
        );
    }

    /// Forget the prompts the agent already read, so that waiting for a prompt waits for a new
    /// one.
    fn skip_prompts(&mut self) {
        self.fixture.herdr.mark();
        self.prompt_offset = self.fixture.herdr.prompts().len();
    }

    /// The prompts the agent read once the reviewer sent every prompt it was to send by now.
    fn settled_prompts(&self) -> Vec<String> {
        self.fixture.settle();
        self.fixture.herdr.prompts()
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
    let explore_prompts = flow
        .settled_prompts()
        .iter()
        .filter(|prompt| prompt.contains("Explore request: "))
        .count();
    assert_eq!(explore_prompts, 1);
    flow.finish();
}

#[test]
fn a_kickoff_the_agent_does_not_start_on_waits_for_a_retry_of_the_same_request() {
    // Herdr gives up on the agent at once, in place of its own 5 seconds.
    let mut flow = ExploreFlow::start_on(ReviewFlowFixture::start(RepoType::Git, |setup| {
        setup.agents = setup
            .agents
            .clone()
            .with_prompt_start_timeout(Duration::from_millis(200));
    }));
    flow.fixture.herdr.swallow_prompts(true);
    let request = flow.exploration.request(None, None).unwrap();
    flow.fixture
        .explore(ExploreCommand::Turn(Box::new(request.clone())));

    flow.wait_for_failure(&request);
    flow.wait_for_dispatch(&request, &review_explore::DispatchState::NotStarted);

    flow.skip_prompts();
    flow.fixture.herdr.swallow_prompts(false);
    // An agent that works already starts on the prompt at once, within Herdr's short wait.
    flow.native_status(herdr_client::protocol::AgentStatus::Working);
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
    flow.fixture.herdr.start_agent(None, AgentLifecycle::Native);
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
