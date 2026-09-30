//! ADR-0001 sequencing rules, driven through the in-memory agent port.

use std::sync::{Arc, Mutex};

use herdr_client::memory::{InMemoryAgents, SentPrompt};
use herdr_client::protocol::{AgentSession, AgentStatus, TabId, WorkspaceId};
use review_mcp::{Operation, Response};
use review_source::{AnchorKind, DiffRangeAnchor};
use review_threads::{MessageId, ReviewThread, ThreadId};
use tempfile::TempDir;

use super::*;

const UNIT: &str = "review";

/// One service over a temporary store and an in-memory agent host.
struct Service {
    state: State,
    agents: InMemoryAgents,
    target: AgentTarget,
    errors: Arc<Mutex<Vec<String>>>,
    seen_prompts: usize,
    _store: TempDir,
}

impl Service {
    fn start() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let store = ReviewStore::open(directory.path().join("state"), directory.path()).unwrap();
        let agents = InMemoryAgents::default();
        agents.upsert_agent(agent("first", "session-1"));
        let target = AgentTarget::new(workspace(), Some(pane("first")));
        let errors = Arc::new(Mutex::new(Vec::new()));
        let published = errors.clone();
        let publish = move |event| {
            if let Event::Error(error) = event {
                published.lock().unwrap().push(error);
            }
        };
        let mut state = State::new(
            store,
            Box::new(agents.clone()),
            target.clone(),
            true,
            Box::new(publish),
        );
        state.command(Command::Thread(ThreadCommand::Load(UNIT.into())));
        Self {
            state,
            agents,
            target,
            errors,
            seen_prompts: 0,
            _store: directory,
        }
    }

    fn thread(&mut self, command: ThreadCommand) {
        self.state.command(Command::Thread(command));
    }

    fn start_thread(&mut self, text: &str) -> ThreadId {
        let post = Post::start(anchor(), "+original".into(), text.into());
        let thread = post.thread_id().clone();
        self.thread(ThreadCommand::Post {
            review_unit: UNIT.into(),
            post,
        });
        thread
    }

    fn comment(&mut self, thread: &ThreadId, text: &str) {
        self.thread(ThreadCommand::Post {
            review_unit: UNIT.into(),
            post: Post::reply(thread.clone(), text.into()),
        });
    }

    fn resolve(&mut self, thread: &ThreadId, resolution: Resolution) {
        self.thread(ThreadCommand::SetResolution {
            review_unit: UNIT.into(),
            thread_id: thread.clone(),
            resolution,
        });
    }

    fn retry(&mut self, thread: &ThreadId) {
        self.thread(ThreadCommand::Retry {
            review_unit: UNIT.into(),
            thread_id: thread.clone(),
        });
    }

    /// Run one delivery pass and return the prompts it submitted.
    fn poll(&mut self) -> Vec<SentPrompt> {
        self.state.poll();
        let prompts = self.agents.prompts();
        let sent = prompts[self.seen_prompts..].to_vec();
        self.seen_prompts = prompts.len();
        sent
    }

    fn fetch(&mut self, access: &str) -> Result<Vec<ReviewThread>, String> {
        match self.state.request(access, &Operation::GetNewMessages)? {
            Response::Threads(threads) => Ok(threads),
            _ => unreachable!("fetching returns threads"),
        }
    }

    fn answer(&mut self, access: &str, thread: &ReviewThread) -> Result<(), String> {
        let in_reply_to = thread.last_comment().unwrap().id.clone();
        let post = Post::answer(
            thread.id.clone(),
            MessageId::parse(&uuid::Uuid::new_v4().to_string()).unwrap(),
            "Done".into(),
            in_reply_to,
        );
        self.state
            .request(access, &Operation::Reply(post))
            .map(|_| ())
    }

    fn observe_agent(&mut self, pane_id: &str, released: bool) {
        self.state
            .command(Command::Observe(HerdrEvent::AgentDetected {
                pane_id: pane(pane_id),
                workspace_id: workspace(),
                agent: Some("codex".into()),
                released,
                final_status: None,
            }));
    }

    /// Whether any notification or wakeup is still armed for delivery.
    fn has_armed_wakeups(&self) -> bool {
        !self.state.notifications.is_empty() || !self.state.wakeups.is_empty()
    }

    fn errors(&self) -> Vec<String> {
        self.errors.lock().unwrap().clone()
    }
}

/// The single wakeup a delivery pass sent: its pane and review access value.
fn only_wakeup(prompts: &[SentPrompt]) -> (String, String) {
    let [prompt] = prompts else {
        panic!("expected exactly one wakeup, got {prompts:?}");
    };
    let access = prompt
        .text
        .split('`')
        .nth(1)
        .expect("the wakeup carries a review access value");
    (prompt.pane_id.0.clone(), access.to_owned())
}

#[test]
fn each_post_notifies_the_agent_once() {
    let mut service = Service::start();
    let thread = service.start_thread("Rename this");

    let (pane, _) = only_wakeup(&service.poll());
    assert_eq!(pane, "first");
    assert!(service.poll().is_empty());
    assert!(service.poll().is_empty());

    service.comment(&thread, "And document it");
    only_wakeup(&service.poll());
    assert!(service.poll().is_empty());
    assert!(service.errors().is_empty());
}

#[test]
fn retry_rearms_an_attempted_notification() {
    let mut service = Service::start();
    let thread = service.start_thread("Rename this");
    let (_, first) = only_wakeup(&service.poll());
    assert!(service.poll().is_empty());

    service.retry(&thread);
    let (pane, retried) = only_wakeup(&service.poll());
    assert_eq!((pane.as_str(), retried), ("first", first));
    assert!(service.poll().is_empty());
}

#[test]
fn reading_never_acknowledges_but_a_reply_does() {
    let mut service = Service::start();
    let thread = service.start_thread("Rename this");
    let (_, access) = only_wakeup(&service.poll());

    let fetched = service.fetch(&access).unwrap();
    assert_eq!(service.fetch(&access).unwrap(), fetched);
    service.retry(&thread);
    only_wakeup(&service.poll());

    service.answer(&access, &fetched[0]).unwrap();
    assert!(service.fetch(&access).unwrap().is_empty());
    service.retry(&thread);
    assert!(service.poll().is_empty());
    assert_eq!(
        service.errors(),
        ["This thread has no unanswered, unresolved comments"]
    );
}

#[test]
fn the_same_agent_session_reuses_its_access_grant() {
    let mut service = Service::start();
    let thread = service.start_thread("Rename this");
    let (_, first) = only_wakeup(&service.poll());

    let mut working = agent("first", "session-1");
    working.agent_status = AgentStatus::Working;
    service.agents.upsert_agent(working);
    service.comment(&thread, "And document it");
    let (_, second) = only_wakeup(&service.poll());

    assert_eq!(second, first);
    assert_eq!(service.fetch(&first).unwrap().len(), 1);
}

#[test]
fn process_identity_grants_access_to_an_agent_without_a_native_session() {
    let mut service = Service::start();
    let mut sessionless = agent("first", "unused");
    sessionless.agent_session = None;
    service.agents.upsert_agent(sessionless.clone());
    service.agents.set_process_group(&pane("first"), 42);
    service.start_thread("Rename this");
    let (_, access) = only_wakeup(&service.poll());
    assert_eq!(service.fetch(&access).unwrap().len(), 1);

    service.agents.set_process_group(&pane("first"), 43);
    assert!(service.fetch(&access).is_err());
}

#[test]
fn focusing_another_agent_hands_the_review_over() {
    let mut service = Service::start();
    let thread = service.start_thread("Rename this");
    let (_, previous) = only_wakeup(&service.poll());

    service.agents.upsert_agent(agent("second", "session-2"));
    service.target.observe_focus(&pane("second"));
    service.comment(&thread, "And document it");
    let (pane, next) = only_wakeup(&service.poll());

    assert_eq!(pane, "second");
    assert_ne!(next, previous);
    assert_eq!(service.fetch(&next).unwrap().len(), 1);
    assert_eq!(
        service.fetch(&previous),
        Err("Unknown review access value; use the value in the latest reviewer wakeup".into())
    );
}

#[test]
fn a_replaced_session_in_the_same_pane_needs_fresh_access() {
    let mut service = Service::start();
    let thread = service.start_thread("Rename this");
    let (_, previous) = only_wakeup(&service.poll());

    service.agents.upsert_agent(agent("first", "session-2"));
    assert!(service.fetch(&previous).is_err());
    service.retry(&thread);
    let (pane, next) = only_wakeup(&service.poll());

    assert_eq!(pane, "first");
    assert_ne!(next, previous);
    assert_eq!(service.fetch(&next).unwrap().len(), 1);
}

#[test]
fn resolving_the_thread_cancels_its_pending_wakeup() {
    let mut service = Service::start();
    let thread = service.start_thread("Rename this");
    service.resolve(&thread, Resolution::Resolved);
    assert!(!service.has_armed_wakeups());
    assert!(service.poll().is_empty());

    service.resolve(&thread, Resolution::Open);
    let (_, access) = only_wakeup(&service.poll());

    // A released agent re-arms its wakeup; resolving first cancels it.
    service.observe_agent("first", true);
    assert!(service.has_armed_wakeups());
    service.resolve(&thread, Resolution::Resolved);
    assert!(!service.has_armed_wakeups());
    assert!(service.poll().is_empty());
    assert!(service.fetch(&access).unwrap().is_empty());
    assert!(service.errors().is_empty());
}

fn workspace() -> WorkspaceId {
    WorkspaceId("workspace".into())
}

fn pane(id: &str) -> PaneId {
    PaneId(id.into())
}

fn agent(pane_id: &str, session: &str) -> Agent {
    Agent {
        pane_id: pane(pane_id),
        tab_id: TabId("tab".into()),
        workspace_id: workspace(),
        name: None,
        display_agent: None,
        agent: Some("codex".into()),
        agent_status: AgentStatus::Idle,
        agent_session: Some(AgentSession {
            source: "herdr:codex".into(),
            agent: "codex".into(),
            kind: "id".into(),
            value: session.into(),
        }),
        cwd: None,
    }
}

fn anchor() -> DiffRangeAnchor {
    DiffRangeAnchor {
        source_checkpoint: "initial".into(),
        old_path: None,
        new_path: Some("lib.rs".into()),
        old_lines: None,
        new_lines: Some(0..1),
        target_kind: AnchorKind::Lines,
        source_hunk_count: 1,
        old_content: None,
        new_content: Some(b"original\n".to_vec()),
        diff_hash: "hash".into(),
    }
}
