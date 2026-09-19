use super::*;
use herdr_client::protocol::AgentStatus;

impl ExploreFlow {
    pub(super) fn conclude(&mut self) -> serde_json::Value {
        self.fixture
            .herdr
            .report_session("conclusion-native-session");
        let question = self.exploration.questions.last().cloned().unwrap();
        let request = self
            .exploration
            .request(
                Some(AnswerInput {
                    option: Some("keep".into()),
                    ..Default::default()
                }),
                Some(&question),
            )
            .unwrap();
        self.fixture
            .commands
            .send(WorkerCommand::Explore(ExploreCommand::Turn(Box::new(
                request.clone(),
            ))))
            .unwrap();
        self.wait_for_prompt(&request);
        let input = serde_json::json!({
            "review": request.instance, "instance": request.instance, "request": request.request, "checkpoint": request.checkpoint,
            "interpretation": {"answer":request.answer.unwrap().id,"status":"accepted","recap":"Recorded: keep resolved.","follow_ups":[]},
            "summary": "Keep the current policy. Further human file inspection is required.",
            "to_be_implemented": "Add regression coverage.",
            "future_work": "Revisit notifications later."
        });
        let accepted = self.call("submit_conclusion", input.clone());
        assert_ne!(accepted.is_error, Some(true), "{accepted:?}");
        input
    }

    fn wait_for_implementation(&self) -> ui_events::ExploreImplementationFinished {
        loop {
            let event = self
                .fixture
                .messages
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
            if let Some(event) = event.downcast_ref::<ui_events::ExploreImplementationFinished>() {
                return event.clone();
            }
        }
    }
}

#[test]
fn conclusion_uses_its_own_mcp_contract_and_implement_waits_on_the_shared_delivery_queue() {
    let mut flow = ExploreFlow::start(RepoType::Git);
    flow.turn(None, 1);
    let payload = flow.conclude();
    assert_eq!(
        flow.exploration.topics["topic1"].status,
        TopicStatus::Accepted
    );
    let update =
        serde_json::to_value(&flow.exploration.conversation.last().unwrap().update).unwrap();
    let rejected = flow.submit(&update);
    assert_eq!(
        rejected.is_error,
        Some(true),
        "question tool cannot close the interview"
    );
    let retry = flow.call("submit_conclusion", payload.clone());
    assert!(
        retry.content[0]
            .as_text()
            .unwrap()
            .text
            .contains("\"applied\":false")
    );
    let mut invalid = payload.clone();
    invalid["to_be_implemented"] = "changed".into();
    assert_eq!(flow.call("submit_conclusion", invalid).is_error, Some(true));
    let before =
        fs::read_to_string(flow.fixture.herdr.directory.path().join("prompt.txt")).unwrap();
    assert_eq!(
        before.len(),
        flow.prompt_offset,
        "conclusion cannot start implementation itself"
    );
    flow.native_status(AgentStatus::Working);
    let request = flow
        .exploration
        .implementation("Only this edited task — preserve\n{{TURN}} literally.".into())
        .unwrap();
    flow.fixture
        .commands
        .send(WorkerCommand::Explore(ExploreCommand::Implement(
            request.clone(),
        )))
        .unwrap();
    // This round trip confirms the command reached the serial owner while the agent was busy.
    flow.call("submit_conclusion", payload);
    assert_eq!(
        fs::read_to_string(flow.fixture.herdr.directory.path().join("prompt.txt")).unwrap(),
        before
    );
    flow.native_status(AgentStatus::Idle);
    let delivered = flow.wait_for_implementation();
    assert_eq!(delivered.request, request);
    assert_eq!(delivered.state, review_explore::DispatchState::Delivered);
    let text = fs::read_to_string(flow.fixture.herdr.directory.path().join("prompt.txt")).unwrap();
    let prompt = &text[before.len()..];
    assert!(prompt.contains(&request.text));
    assert!(
        !prompt.contains("Revisit notifications") && !prompt.contains("Add regression coverage")
    );
    flow.finish();
}

#[test]
fn cancelled_implementation_is_not_delivered_when_the_agent_becomes_ready() {
    let mut flow = ExploreFlow::start(RepoType::Git);
    flow.turn(None, 1);
    flow.conclude();
    flow.native_status(AgentStatus::Working);
    let request = flow
        .exploration
        .implementation("Cancelled task".into())
        .unwrap();
    flow.fixture
        .commands
        .send(WorkerCommand::Explore(ExploreCommand::Implement(request)))
        .unwrap();
    flow.fixture
        .commands
        .send(WorkerCommand::Explore(ExploreCommand::CancelImplementation))
        .unwrap();
    assert_eq!(
        flow.wait_for_implementation().state,
        review_explore::DispatchState::Cancelled
    );
    flow.native_status(AgentStatus::Idle);
    assert_eq!(
        fs::read_to_string(flow.fixture.herdr.directory.path().join("prompt.txt"))
            .unwrap()
            .len(),
        flow.prompt_offset
    );
    flow.finish();
}
