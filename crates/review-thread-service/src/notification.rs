/// A pending notification waiting for the active native agent to be ready.
#[derive(Default)]
pub(super) struct Notification {
    pub(super) retry: bool,
}
