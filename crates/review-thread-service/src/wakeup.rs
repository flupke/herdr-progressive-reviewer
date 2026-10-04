/// Track which posted comments have already triggered a notification attempt.
#[derive(Default)]
pub(super) struct Wakeup {
    requested_through: u64,
    notified_through: u64,
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
    }

    /// Record that the notification for the requested comments is on its way, and return the
    /// sequence it covers.
    pub(super) fn sent(&mut self) -> u64 {
        self.notified_through = self.requested_through;
        self.notified_through
    }

    /// Whether nothing requested or re-armed this wakeup since a notification `sent` through
    /// that sequence.
    pub(super) fn unchanged_since(&self, sent: u64) -> bool {
        self.requested_through == sent && self.notified_through == sent
    }

    pub(super) fn answered(&mut self) {
        self.requested_through = 0;
        self.notified_through = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_new_comment_notifies_once_without_waiting_for_an_agent_turn() {
        let mut wakeup = Wakeup::default();
        wakeup.request(1, false);
        assert!(wakeup.needs_poll());
        wakeup.sent();
        wakeup.request(1, false);
        assert!(!wakeup.needs_poll());
        wakeup.request(2, false);
        assert!(wakeup.needs_poll());
        wakeup.sent();
        assert!(!wakeup.needs_poll());
    }

    #[test]
    fn a_wakeup_requested_after_a_notification_was_sent_is_not_the_one_sent() {
        let mut wakeup = Wakeup::default();
        wakeup.request(1, false);
        let sent = wakeup.sent();
        assert!(wakeup.unchanged_since(sent));

        wakeup.request(2, false);
        assert!(!wakeup.unchanged_since(sent));
        let sent = wakeup.sent();
        wakeup.request(2, true);
        assert!(!wakeup.unchanged_since(sent));
    }

    #[test]
    fn retry_and_session_resumption_rearm_unanswered_comments() {
        let mut wakeup = Wakeup::default();
        wakeup.request(1, false);
        wakeup.sent();
        wakeup.request(1, true);
        assert!(wakeup.needs_poll());
        wakeup.sent();
        wakeup.interrupted();
        assert!(wakeup.needs_poll());
        wakeup.answered();
        wakeup.interrupted();
        assert!(!wakeup.needs_poll());
    }
}
