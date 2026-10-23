//! The prompts the reviewer types to an agent that runs the hooks of the reviewer's Claude Code
//! plugin: the agent's hook confirms each one, and blocks text left in its input box that joined
//! it.

use review_explore::DispatchState;

use super::super::run_ahead::StandIn;
use super::*;

/// A round whose agent is a Claude Code stand-in that runs the plugin's hooks, and whose prompts
/// Herdr gives up on at once, in place of its own wait: only the hook confirms them in time.
fn hooked_flow() -> ExploreFlow {
    let repository_files = repository_fixture(RepoType::Git);
    repository_files.write("reviewed.rs", b"pub fn reviewed() {}\n");
    let herdr =
        IsolatedHerdrServer::start_forkable(repository_files.root(), "session", StandIn::Forkable);
    ExploreFlow::start_on(ReviewFlowFixture::start_on(
        repository_files,
        herdr,
        |setup| {
            setup.agents = setup
                .agents
                .clone()
                .with_prompt_start_timeout(Duration::from_millis(200));
        },
    ))
}

#[test]
fn a_prompt_the_agents_hook_reports_is_delivered_though_herdr_never_sees_the_agent_start() {
    let mut flow = hooked_flow();
    flow.fixture.herdr.swallow_prompts(true);
    let request = flow.exploration.request(None, None).unwrap();

    flow.fixture
        .explore(ExploreCommand::Turn(Box::new(request.clone())));

    flow.wait_for_prompt(&request);
    flow.wait_for_dispatch(&request, &DispatchState::Delivered);
    flow.finish();
}

#[test]
fn text_left_in_the_input_box_that_joins_a_prompt_is_blocked_and_retry_sends_the_prompt_alone() {
    let mut flow = hooked_flow();
    let herdr = &flow.fixture.herdr;
    herdr.run_cli(&["pane", "send-text", &herdr.pane_id.0, "half a thought "]);
    let request = flow.exploration.request(None, None).unwrap();

    flow.fixture
        .explore(ExploreCommand::Turn(Box::new(request.clone())));

    let blocked = flow
        .fixture
        .herdr
        .events()
        .wait_for("the blocked prompt", |reported| {
            matches!(reported.event, StandInEvent::PromptBlocked { .. })
        });
    let StandInEvent::PromptBlocked { text } = blocked.event else {
        unreachable!()
    };
    assert!(text.starts_with("half a thought Explore round "), "{text}");
    flow.wait_for_failure(&request);
    let error = flow.state.wait_until(
        "the turn not sent",
        || match &flow.saved().turns[&request.request].state {
            DispatchState::NotSent(error) => Some(error.clone()),
            _ => None,
        },
        || format!("the turn stayed {:?}", flow.saved().turns[&request.request]),
    );
    assert!(error.ends_with(": half a thought"), "{error}");
    // The agent put the joined prompt back in its input box, which its user empties.
    let herdr = &flow.fixture.herdr;
    herdr.run_cli(&["pane", "send-keys", &herdr.pane_id.0, "ctrl+u"]);
    assert!(
        flow.settled_prompts().is_empty(),
        "the agent took no turn on the joined prompt"
    );
    flow.fixture
        .explore(ExploreCommand::Retry(Box::new(request.clone())));

    flow.wait_for_prompt(&request);
    flow.wait_for_dispatch(&request, &DispatchState::Delivered);
    let prompts = flow.settled_prompts();
    assert_eq!(prompts.len(), 1, "{prompts:?}");
    assert!(!prompts[0].contains("half a thought"), "{prompts:?}");
    flow.finish();
}
