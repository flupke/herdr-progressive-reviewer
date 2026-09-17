use super::*;
use review_thread_service::{PinnedAgent, PromptCancellation, PromptReceipt};

impl ConversationFixture {
    fn queue_prompt(&self, text: &str) -> (PromptReceipt, PromptCancellation) {
        let agent = self
            .target
            .clone()
            .resolve(&self.server.client())
            .unwrap()
            .unwrap();
        self.worker
            .as_ref()
            .unwrap()
            .prompt_sender()
            .send(PinnedAgent::new(agent), text.into())
    }

    fn wait_for_prompt_text(&self, text: &str) {
        let deadline = Instant::now() + Duration::from_secs(5);
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
fn structured_prompts_wait_for_focus_and_drafts_then_send_once() {
    let fixture = ConversationFixture::start("codex");
    let client = fixture.server.client();
    client.focus_agent(&fixture.server.pane_id).unwrap();
    let (receipt, _cancellation) = fixture.queue_prompt("Queued reviewer turn");
    thread::sleep(Duration::from_millis(350));
    assert!(fixture.prompts().is_empty());

    client
        .send_text(&fixture.server.pane_id, "My unposted draft")
        .unwrap();
    fixture.wait_for_screen(|screen| screen.contains("My unposted draft"));
    fixture.server.run_cli(&[
        "pane",
        "focus",
        "--direction",
        "right",
        "--pane",
        &fixture.server.pane_id.0,
    ]);
    thread::sleep(Duration::from_millis(350));
    assert!(fixture.prompts().is_empty());
    assert!(
        client
            .read_agent_screen(&fixture.server.pane_id)
            .unwrap()
            .contains("My unposted draft")
    );

    client
        .send_keys(&fixture.server.pane_id, &["Enter"])
        .unwrap();
    fixture.wait_for_prompt_text("Queued reviewer turn");
    receipt.wait().unwrap();
    thread::sleep(Duration::from_millis(350));
    assert_eq!(
        fixture.prompts(),
        "My unposted draft\nQueued reviewer turn\n"
    );
}

#[test]
fn queued_prompts_reject_replaced_sessions_and_stop_with_the_worker() {
    let mut fixture = ConversationFixture::start("codex");
    fixture.status(AgentStatus::Working);
    let (receipt, _cancellation) = fixture.queue_prompt("For the original conversation");
    fixture.replace_session("replacement-session");
    let error = receipt.wait().unwrap_err().to_string();
    assert!(
        error.contains("different agent conversation") || error.contains("no longer available"),
        "{error}"
    );
    fixture.status(AgentStatus::Idle);
    thread::sleep(Duration::from_millis(350));
    assert!(fixture.prompts().is_empty());

    fixture.status(AgentStatus::Working);
    let (receipt, _cancellation) = fixture.queue_prompt("Cancelled by shutdown");
    drop(fixture.worker.take());
    assert!(
        receipt
            .wait()
            .unwrap_err()
            .to_string()
            .contains("dispatcher closed")
    );
    fixture.server.report_agent("idle");
    assert!(fixture.prompts().is_empty());
}
