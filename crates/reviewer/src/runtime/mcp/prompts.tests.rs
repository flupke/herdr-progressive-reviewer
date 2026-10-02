use super::*;
use review_thread_service::{PinnedAgent, PromptCancellation, PromptReceipt};

impl ConversationFixture {
    fn pinned_agent(&self) -> PinnedAgent {
        PinnedAgent::new(
            self.target
                .clone()
                .resolve(&self.server.client())
                .unwrap()
                .unwrap(),
        )
    }

    fn queue_prompt(&self, agent: PinnedAgent, text: &str) -> (PromptReceipt, PromptCancellation) {
        self.worker
            .as_ref()
            .unwrap()
            .prompt_sender()
            .send(agent, text.into())
    }

    fn wait_for_prompt_text(&self, text: &str) {
        let deadline = Instant::now() + crate::runtime::tests::HERDR_WAIT;
        while !self.prompts().contains(text) {
            assert!(
                Instant::now() < deadline,
                "prompt was not delivered: {text}"
            );
            thread::sleep(Duration::from_millis(25));
        }
    }
}

#[test]
fn structured_prompts_submit_once_while_the_agent_is_working_and_focused() {
    let fixture = ConversationFixture::start("codex");
    fixture.status(AgentStatus::Working);
    fixture
        .server
        .client()
        .focus_agent(&fixture.server.pane_id)
        .unwrap();
    let (receipt, _cancellation) = fixture.queue_prompt(fixture.pinned_agent(), "Reviewer turn");
    receipt.wait().unwrap();
    fixture.wait_for_prompt_text("Reviewer turn");
    fixture.status(AgentStatus::Idle);
    thread::sleep(Duration::from_millis(250));
    assert_eq!(fixture.prompts(), "Reviewer turn\n");
}

#[test]
fn structured_prompts_reject_a_replaced_conversation() {
    let fixture = ConversationFixture::start("codex");
    let original = fixture.pinned_agent();
    fixture.replace_session("replacement-session");
    let (receipt, _cancellation) = fixture.queue_prompt(original, "For the original conversation");
    let error = receipt.wait().unwrap_err().to_string();
    assert!(error.contains("different agent conversation"), "{error}");
    assert!(fixture.prompts().is_empty());
}

#[test]
fn selected_prompt_uses_the_current_pane_when_native_identity_changes_before_delivery() {
    let fixture = ConversationFixture::start("codex");
    let selected = PinnedAgent::for_selected_prompt(
        fixture
            .target
            .clone()
            .resolve(&fixture.server.client())
            .unwrap()
            .unwrap(),
    );
    fixture.replace_session("");
    let (receipt, _cancellation) = fixture.queue_prompt(selected.clone(), "Selected turn");
    receipt.wait().unwrap();
    fixture.wait_for_prompt_text("Selected turn");
    assert!(
        selected
            .current(&fixture.server.client())
            .unwrap()
            .is_some()
    );
}

#[test]
fn selected_prompt_freezes_the_native_identity_used_at_delivery() {
    let fixture = ConversationFixture::start("codex");
    let selected = PinnedAgent::for_selected_prompt(
        fixture
            .target
            .clone()
            .resolve(&fixture.server.client())
            .unwrap()
            .unwrap(),
    );
    fixture.replace_session("chosen-conversation");
    let (receipt, _cancellation) = fixture.queue_prompt(selected.clone(), "Chosen turn");
    receipt.wait().unwrap();
    fixture.replace_session("another-conversation");
    assert!(selected.current(&fixture.server.client()).is_err());
}

#[test]
fn cancelling_or_closing_delivery_waiting_for_session_identity_sends_nothing() {
    let mut fixture = ConversationFixture::start("codex");
    let original = fixture.pinned_agent();
    fixture.replace_session("");
    let (receipt, cancellation) = fixture.queue_prompt(original.clone(), "Cancelled turn");
    drop(cancellation);
    assert!(matches!(
        receipt.wait(),
        Err(review_thread_service::PromptError::Cancelled)
    ));

    let (receipt, _cancellation) = fixture.queue_prompt(original, "Cancelled by shutdown");
    drop(fixture.worker.take());
    assert!(
        receipt
            .wait()
            .unwrap_err()
            .to_string()
            .contains("dispatcher closed")
    );
    fixture.server.report_session("session");
    assert!(fixture.prompts().is_empty());
}
