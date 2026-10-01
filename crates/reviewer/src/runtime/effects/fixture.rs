//! The one way tests start [`Effects`]: against a temporary repository, with results
//! collected from the same channels the runtime reads.

use std::time::{Duration, Instant};

use component_core::{ApplicationEvent, EventEnvelope};
use crossbeam_channel::{Sender as EventSender, unbounded};
use herdr_client::client::HerdrClient;
use herdr_client::protocol::{AgentTarget, WorkspaceId};
use review_repository::repository::{RepoType, Repository};
use review_significance::JevClassifier;
use review_source::ReviewCheckpoint;
use review_store::ReviewStore;
use review_test_support::{ReviewRepositoryFixture, repository_fixture};
use review_ui::{Action, Theme};
use ui_events::{RepositoryMetadataChanged, RepositoryRefreshFinished};

use super::{Effects, Outputs, Setup};
use crate::runtime::events;

const EVENT_TIMEOUT: Duration = Duration::from_secs(10);

pub(in crate::runtime) struct EffectsFixture {
    // Declared first so the workers stop before their repository disappears.
    pub(in crate::runtime) effects: Effects,
    pub(in crate::runtime) inbox: events::Inbox,
    pub(in crate::runtime) background: EventSender<EventEnvelope>,
    pub(in crate::runtime) interactive: EventSender<EventEnvelope>,
    pub(in crate::runtime) repository: Repository,
    pub(in crate::runtime) store: ReviewStore,
    pub(in crate::runtime) files: Box<dyn ReviewRepositoryFixture>,
    pub(in crate::runtime) state: tempfile::TempDir,
}

impl EffectsFixture {
    /// Effects for a fresh repository that never reach an agent.
    pub(in crate::runtime) fn new(kind: RepoType) -> Self {
        Self::start(repository_fixture(kind), |_| {})
    }

    /// Effects for `files`, after `configure` adjusts the offline setup.
    pub(in crate::runtime) fn start(
        files: Box<dyn ReviewRepositoryFixture>,
        configure: impl FnOnce(&mut Setup),
    ) -> Self {
        let state = tempfile::tempdir().unwrap();
        let repository = Repository::discover(files.root())
            .unwrap()
            .with_state_root(state.path());
        let store = ReviewStore::open(state.path(), repository.root()).unwrap();
        let mut setup = Setup {
            repository: repository.clone(),
            store: store.clone(),
            target: AgentTarget::new(WorkspaceId("test".into()), None),
            agents: HerdrClient::new(
                "/nonexistent/reviewer-test.sock".into(),
                "reviewer-test".into(),
                "/nonexistent".into(),
            ),
            endpoint: Err("No MCP listener in this unit test".into()),
            theme: Theme::default(),
            jev: JevClassifier::disabled(),
            source_watches: None,
        };
        configure(&mut setup);
        let offline = setup.endpoint.is_err();
        let (background, background_events) = unbounded();
        let (interactive, interactive_events) = unbounded();
        let effects = Effects::start(
            setup,
            &Outputs {
                background: background.clone(),
                interactive: interactive.clone(),
            },
        );
        let mut fixture = Self {
            effects,
            inbox: events::Inbox::new(background_events, interactive_events),
            background,
            interactive,
            repository,
            store,
            files,
            state,
        };
        if offline {
            // Without an endpoint, the conversation worker reports MCP as unavailable
            // once; tests start after that notice.
            fixture.wait_for::<ui_events::ToastRequested>();
        }
        fixture
    }

    /// Perform actions that never reach the terminal.
    pub(in crate::runtime) fn perform(&self, actions: impl IntoIterator<Item = Action>) {
        let flow = self
            .effects
            .perform_all(actions.into_iter().collect(), &mut |action| {
                panic!("unexpected terminal action {action:?}")
            })
            .unwrap();
        assert!(flow.is_continue());
    }

    /// Refresh and return every event up to the end of the refresh.
    pub(in crate::runtime) fn refresh(&mut self) -> Vec<EventEnvelope> {
        self.effects.refresh().unwrap();
        self.events_until::<RepositoryRefreshFinished>()
    }

    /// Refresh and return the checkpoint it published.
    pub(in crate::runtime) fn refreshed_checkpoint(&mut self) -> ReviewCheckpoint {
        self.refresh()
            .iter()
            .find_map(|event| event.downcast_ref::<RepositoryMetadataChanged>())
            .expect("the refresh publishes the checkpoint")
            .review_checkpoint
            .clone()
    }

    /// The next event, or `None` after `timeout`.
    pub(in crate::runtime) fn recv_timeout(&mut self, timeout: Duration) -> Option<EventEnvelope> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(event) = self.inbox.try_recv() {
                return Some(event);
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Every event already delivered.
    pub(in crate::runtime) fn drain_events(&mut self) -> Vec<EventEnvelope> {
        std::iter::from_fn(|| self.inbox.try_recv()).collect()
    }

    pub(in crate::runtime) fn next_event(&mut self) -> EventEnvelope {
        self.recv_timeout(EVENT_TIMEOUT).expect("no event arrived")
    }

    /// Every event up to and including the first `E`.
    pub(in crate::runtime) fn events_until<E: ApplicationEvent>(&mut self) -> Vec<EventEnvelope> {
        let mut events = Vec::new();
        loop {
            let event = self.next_event();
            let found = event.downcast_ref::<E>().is_some();
            events.push(event);
            if found {
                return events;
            }
        }
    }

    /// The next `E`, skipping other events.
    pub(in crate::runtime) fn wait_for<E: ApplicationEvent + Clone>(&mut self) -> E {
        let event = self.events_until::<E>().pop().unwrap();
        event.downcast_ref::<E>().unwrap().clone()
    }
}
