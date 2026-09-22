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

    pub(super) fn sent(&mut self) {
        self.notified_through = self.requested_through;
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
