use herdr_client::protocol::AgentStatus;

/// Coalesce idle wakeups while an agent picks up new comments through MCP.
#[derive(Default)]
pub(super) struct Wakeup {
    requested_through: u64,
    notified_through: u64,
    in_flight: bool,
    working_seen: bool,
    deferred_notice: bool,
}

impl Wakeup {
    pub(super) fn needs_poll(&self) -> bool {
        self.requested_through > self.notified_through
    }

    pub(super) fn request(&mut self, sequence: u64, retry: bool) {
        self.requested_through = sequence;
        if retry {
            self.interrupted();
        }
    }

    pub(super) fn interrupted(&mut self) {
        self.notified_through = 0;
        self.in_flight = false;
        self.working_seen = false;
    }

    pub(super) fn observe(&mut self, status: AgentStatus) {
        match status {
            AgentStatus::Working => self.working_seen = true,
            AgentStatus::Idle | AgentStatus::Done if self.working_seen => {
                self.in_flight = false;
                self.working_seen = false;
            }
            _ => {}
        }
    }

    pub(super) fn should_notify(&self, status: AgentStatus, unread: bool) -> bool {
        unread
            && !self.in_flight
            && self.requested_through > self.notified_through
            && matches!(status, AgentStatus::Idle | AgentStatus::Done)
    }

    pub(super) fn sent(&mut self, succeeded: bool) {
        self.deferred_notice = false;
        self.notified_through = self.requested_through;
        self.in_flight = succeeded;
    }

    pub(super) fn answered(&mut self) {
        self.deferred_notice = false;
        self.requested_through = 0;
        self.notified_through = 0;
        self.in_flight = false;
    }

    pub(super) fn defer(&mut self) -> bool {
        !std::mem::replace(&mut self.deferred_notice, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_active_agent_is_not_interrupted_and_late_comments_wake_it_on_idle() {
        let mut wakeup = Wakeup::default();
        wakeup.request(1, false);
        assert!(wakeup.should_notify(AgentStatus::Idle, true));
        wakeup.sent(true);
        wakeup.request(2, false);
        assert!(!wakeup.should_notify(AgentStatus::Idle, true));
        wakeup.observe(AgentStatus::Working);
        assert!(!wakeup.should_notify(AgentStatus::Working, true));
        wakeup.observe(AgentStatus::Idle);
        assert!(wakeup.should_notify(AgentStatus::Idle, true));
        assert!(!wakeup.should_notify(AgentStatus::Idle, false));
    }

    #[test]
    fn a_failed_wakeup_can_be_retried_explicitly() {
        let mut wakeup = Wakeup::default();
        wakeup.request(1, false);
        wakeup.sent(false);
        assert!(!wakeup.should_notify(AgentStatus::Idle, true));
        wakeup.request(1, true);
        assert!(wakeup.should_notify(AgentStatus::Idle, true));
        wakeup.sent(true);
        wakeup.request(1, true);
        assert!(wakeup.should_notify(AgentStatus::Idle, true));
        wakeup.observe(AgentStatus::Working);
        wakeup.answered();
        wakeup.observe(AgentStatus::Idle);
        assert!(!wakeup.should_notify(AgentStatus::Idle, true));
    }

    #[test]
    fn resuming_retries_an_unread_wakeup_but_not_answered_comments() {
        let mut wakeup = Wakeup::default();
        wakeup.request(1, false);
        wakeup.sent(true);
        wakeup.interrupted();
        assert!(!wakeup.should_notify(AgentStatus::Working, true));
        assert!(wakeup.should_notify(AgentStatus::Idle, true));
        wakeup.sent(true);
        wakeup.answered();
        wakeup.interrupted();
        assert!(!wakeup.should_notify(AgentStatus::Idle, false));
    }

    #[test]
    fn an_unread_notification_does_not_loop_when_the_agent_cannot_use_mcp() {
        let mut wakeup = Wakeup::default();
        wakeup.request(2, false);
        wakeup.sent(true);
        assert!(
            !wakeup.needs_poll(),
            "announced work must not keep an idle poll alive"
        );
        wakeup.observe(AgentStatus::Working);
        wakeup.observe(AgentStatus::Idle);
        assert!(!wakeup.should_notify(AgentStatus::Idle, true));
        // An explicit retry can deliver the same comments after MCP is repaired.
        wakeup.request(2, true);
        assert!(wakeup.should_notify(AgentStatus::Idle, true));
        wakeup.sent(true);
        wakeup.observe(AgentStatus::Working);
        wakeup.observe(AgentStatus::Done);
        assert!(!wakeup.should_notify(AgentStatus::Done, true));
        wakeup.observe(AgentStatus::Working);
        wakeup.answered();
        wakeup.observe(AgentStatus::Idle);
        assert!(!wakeup.should_notify(AgentStatus::Idle, true));
        wakeup.request(3, false);
        assert!(wakeup.should_notify(AgentStatus::Idle, true));
    }
}
