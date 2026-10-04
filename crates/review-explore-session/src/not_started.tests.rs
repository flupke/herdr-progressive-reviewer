//! Prompts the agent did not start on: Herdr wrote them into its pane, and nothing happened.

use review_explore::{Command, DispatchState};
use review_explore_page::{ImplementationState, Interruption, RoundStage};
use review_thread_service::PromptError;
use ui_events::ExploreProgress;

use super::*;

impl Harness {
    /// The agent reads each prompt without starting on it, or starts on each prompt again.
    fn swallow_prompts(&self, swallow: bool) {
        self.agents.swallow_prompts(&PaneId(PANE.into()), swallow);
    }

    /// Hand the session the outcome of its prompt, as its owner does, and return the failure
    /// the front ends hear of.
    fn prompt_failed(&mut self) -> String {
        let input = self.inbox.recv_timeout(Duration::from_secs(10)).unwrap();
        self.session.handle(input);
        self.next::<ui_events::ExploreFinished>()
            .result
            .expect_err("the prompt failed")
    }
}

#[test]
fn a_turn_the_agent_does_not_start_on_waits_for_a_retry_of_the_same_request() {
    let mut harness = Harness::start();
    harness.capture();
    let kickoff = harness.request(None);
    harness.swallow_prompts(true);

    harness
        .session
        .handle(Input::Command(Command::Turn(Box::new(kickoff.clone()))));

    assert!(harness.next::<ui_events::ExplorePosted>().result.is_ok());
    let failure = harness.prompt_failed();
    assert_eq!(failure, PromptError::NotStarted.to_string());
    assert_eq!(
        harness.saved().turns[&kickoff.request].state,
        DispatchState::NotStarted
    );
    assert_eq!(
        harness.page.stage(),
        RoundStage::Interrupted {
            request: Some(kickoff.request.clone()),
            interruption: Interruption::NotStarted,
        }
    );

    let restored = harness.reopen();
    assert_eq!(restored.progress, ExploreProgress::NotStarted);

    harness.adopt(&restored);
    harness.swallow_prompts(false);
    let retry = harness.exploration.as_mut().unwrap().retry().unwrap();
    assert_eq!(retry.request, kickoff.request);
    // The swallowed prompt counts as delivered to the agent's pane.
    harness.delivered_prompt();
    harness
        .session
        .handle(Input::Command(Command::Retry(Box::new(retry.clone()))));
    assert!(harness.next::<ui_events::ExplorePosted>().result.is_ok());
    let prompt = harness.delivered_prompt();
    assert!(prompt.contains(&format!("Explore request: {}\n", kickoff.request)));
    let access = prompt
        .lines()
        .find_map(|line| line.strip_prefix("Explore review access: "))
        .unwrap()
        .to_owned();
    assert!(applied(harness.submit(&access, question(&retry, 1))));
    assert_eq!(
        harness.saved().turns[&kickoff.request].state,
        DispatchState::Delivered
    );
}

#[test]
fn an_implementation_request_the_agent_does_not_start_on_is_sent_again_as_it_was() {
    let mut harness = Harness::start();
    harness.conclude();
    let request = harness
        .saved()
        .exploration
        .implementation("Add a regression test.".into())
        .unwrap();
    harness.swallow_prompts(true);

    harness
        .session
        .handle(Input::Command(Command::Implement(request.clone())));

    let finished = harness.next::<ui_events::ExploreImplementationFinished>();
    assert_eq!(finished.state, DispatchState::NotStarted);
    harness.session.handle(Input::StorageChanged);
    let saved = harness.saved();
    assert_eq!(
        saved.implementations[&request.delivery].state,
        DispatchState::NotStarted
    );
    let RoundStage::Conclusion {
        implementation: Some(shown),
        ..
    } = harness.page.stage()
    else {
        panic!("the page shows {:?}", harness.page.stage());
    };
    assert_eq!(shown.state, ImplementationState::NotStarted);

    harness.swallow_prompts(false);
    let prompts = harness.agents.prompts().len();
    harness
        .session
        .handle(Input::Command(Command::Implement(request.clone())));
    let finished = harness.next::<ui_events::ExploreImplementationFinished>();
    assert_eq!(finished.state, DispatchState::Delivered);
    assert_eq!(finished.request, request);
    let prompts = &harness.agents.prompts()[prompts..];
    assert_eq!(prompts.len(), 1);
    assert_eq!(
        prompts[0].text,
        review_explore_runner::implementation_prompt(&request)
    );
    assert_eq!(harness.saved().implementations.len(), 1);
}
