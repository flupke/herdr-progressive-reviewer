//! ADR-0001 sequencing rules, driven through the in-memory agent port.

use std::sync::{Arc, Mutex};

use herdr_client::memory::{InMemoryAgents, SentPrompt};
use herdr_client::protocol::{AgentSession, AgentStatus, TabId, WorkspaceId};
use review_mcp::{Operation, Response};
use review_source::{AnchorKind, DiffRangeAnchor};
use review_threads::{AskedUnder, MessageId, ReviewThread, ThreadId, WakeupFailure};
use tempfile::TempDir;

use super::*;

const UNIT: &str = "review";

/// One service over a temporary store and an in-memory agent host.
struct Service {
    state: State,
    /// The outcomes the courier sends back to the worker.
    inputs: std::sync::mpsc::Receiver<Input>,
    agents: InMemoryAgents,
    target: AgentTarget,
    errors: Arc<Mutex<Vec<String>>>,
    /// The outcome of each wakeup, as the worker published it.
    wakeups: Arc<Mutex<Vec<Option<WakeupFailure>>>>,
    /// The messages the worker said the reviewer posted in a round's conversation.
    round_messages: Arc<Mutex<Vec<crate::RoundMessage>>>,
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
        let wakeups = Arc::new(Mutex::new(Vec::new()));
        let round_messages = Arc::new(Mutex::new(Vec::new()));
        let published = errors.clone();
        let outcomes = wakeups.clone();
        let heard = round_messages.clone();
        let publish = move |event| match event {
            Event::RoundMessage(message) => heard.lock().unwrap().push(message),
            Event::Error(error) => published.lock().unwrap().push(error),
            Event::Wakeup { failure, .. } => {
                if let Some(failure) = &failure {
                    published.lock().unwrap().push(failure.error.clone());
                }
                outcomes.lock().unwrap().push(failure);
            }
            _ => {}
        };
        let (sender, inputs) = std::sync::mpsc::channel();
        let mut state = State::new(
            store,
            Arc::new(agents.clone()),
            target.clone(),
            true,
            Box::new(publish),
            sender,
            Arc::default(),
        );
        state.command(Command::Thread(ThreadCommand::Load(UNIT.into())));
        Self {
            state,
            inputs,
            agents,
            target,
            errors,
            wakeups,
            round_messages,
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

    /// Run one delivery pass and return the prompts it submitted, once their outcomes are back.
    fn poll(&mut self) -> Vec<SentPrompt> {
        self.state.poll();
        self.state.courier.flush();
        while let Ok(input) = self.inputs.try_recv() {
            let _ = self.state.input(input);
        }
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

    fn wakeup_outcomes(&self) -> Vec<Option<WakeupFailure>> {
        self.wakeups.lock().unwrap().clone()
    }

    /// Post a message to the conversation of `round` and return its posted identity.
    fn talk(&mut self, round: &str, text: &str, asked_under: Option<AskedUnder>) -> MessageId {
        let post = Post::to_round(round, text.into(), asked_under, Some("a passage".into()));
        let id = post.message().id.clone();
        self.thread(ThreadCommand::Post {
            review_unit: UNIT.into(),
            post,
        });
        id
    }
}

/// The messages the worker said the reviewer posted in `round`'s conversation, oldest first.
fn round_messages(service: &Service, round: &str) -> Vec<MessageId> {
    service
        .round_messages
        .lock()
        .unwrap()
        .iter()
        .filter(|heard| heard.review_unit == ReviewUnit::from(UNIT) && heard.round == round)
        .map(|heard| heard.message.clone())
        .collect()
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

#[test]
fn a_notification_the_agent_does_not_start_on_is_reported_and_sent_again_on_retry() {
    let mut service = Service::start();
    service.agents.swallow_prompts(&pane("first"), true);
    let thread = service.start_thread("Rename this");

    let (_, first) = only_wakeup(&service.poll());

    assert_eq!(service.errors(), [PromptError::NotStarted.to_string()]);
    assert!(service.poll().is_empty());
    service.agents.swallow_prompts(&pane("first"), false);
    service.retry(&thread);
    let (pane, retried) = only_wakeup(&service.poll());
    assert_eq!((pane.as_str(), retried), ("first", first));
    assert_eq!(service.errors().len(), 1);
}

/// The agents, with an agent that takes until the test releases it to start on a prompt.
struct SlowAgents {
    agents: InMemoryAgents,
    prompting: std::sync::mpsc::Sender<()>,
    release: Mutex<std::sync::mpsc::Receiver<()>>,
}

impl AgentPort for SlowAgents {
    fn session_snapshot(&self) -> herdr_client::Result<herdr_client::protocol::SessionSnapshot> {
        self.agents.session_snapshot()
    }

    fn list_agents(&self) -> herdr_client::Result<Vec<Agent>> {
        self.agents.list_agents()
    }

    fn get_agent(&self, pane_id: &PaneId) -> herdr_client::Result<Option<Agent>> {
        self.agents.get_agent(pane_id)
    }

    fn pane_process_info(
        &self,
        pane_id: &PaneId,
    ) -> herdr_client::Result<herdr_client::protocol::PaneProcessInfo> {
        self.agents.pane_process_info(pane_id)
    }

    fn prompt_agent(&self, pane_id: &PaneId, text: &str) -> herdr_client::Result<()> {
        let _ = self.prompting.send(());
        let _ = self.release.lock().unwrap().recv();
        self.agents.prompt_agent(pane_id, text)
    }
}

/// A worker over agents whose agent takes until the test releases it to start on each prompt.
struct SlowWorker {
    worker: crate::Worker,
    agents: InMemoryAgents,
    /// One signal for each prompt on its way to the agent.
    prompted: std::sync::mpsc::Receiver<()>,
    /// Lets the agent start on one prompt.
    release: std::sync::mpsc::Sender<()>,
    events: std::sync::mpsc::Receiver<Event>,
    _store: TempDir,
}

impl SlowWorker {
    fn start() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let store = ReviewStore::open(directory.path().join("state"), directory.path()).unwrap();
        let agents = InMemoryAgents::default();
        agents.upsert_agent(agent("first", "session-1"));
        let (prompting, prompted) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel();
        let (published, events) = std::sync::mpsc::channel();
        let worker = crate::Worker::start(
            store,
            SlowAgents {
                agents: agents.clone(),
                prompting,
                release: Mutex::new(released),
            },
            AgentTarget::new(workspace(), Some(pane("first"))),
            Err("No MCP listener in this test".into()),
            |_| Err("No MCP listener in this test".into()),
            move |event| {
                let _ = published.send(event);
            },
        );
        Self {
            worker,
            agents,
            prompted,
            release,
            events,
            _store: directory,
        }
    }
}

/// The threads the worker loaded next, skipping its other events. The guard fires only when
/// the worker hangs.
fn loaded(events: &std::sync::mpsc::Receiver<Event>) -> ui_events::ReviewThreadsLoaded {
    loop {
        match events.recv_timeout(std::time::Duration::from_secs(10)) {
            Ok(Event::Loaded(loaded)) => return loaded,
            Ok(_) => {}
            Err(error) => panic!("the worker did not load the threads: {error}"),
        }
    }
}

#[test]
fn a_prompt_withdrawn_while_it_waits_behind_another_is_not_sent() {
    let SlowWorker {
        worker,
        agents,
        prompted,
        release,
        events,
        ..
    } = SlowWorker::start();
    let pinned = crate::PinnedAgent::new(agent("first", "session-1"));
    let (first, _first_cancellation) = worker
        .prompt_sender()
        .send(pinned.clone(), "First turn".into());
    prompted
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("the first prompt is on its way");
    let (second, second_cancellation) = worker.prompt_sender().send(pinned, "Second turn".into());
    // The worker hands each prompt to the courier before it takes its next input: once it
    // loads the threads, the second prompt waits behind the first.
    worker.send(Command::Thread(ThreadCommand::Load(UNIT.into())));
    loaded(&events);

    drop(second_cancellation);
    // Released for both, so that a second prompt sent anyway shows instead of waiting.
    release.send(()).unwrap();
    release.send(()).unwrap();

    first.wait().unwrap();
    assert!(matches!(second.wait(), Err(crate::PromptError::Cancelled)));
    let texts: Vec<_> = agents
        .prompts()
        .into_iter()
        .map(|prompt| prompt.text)
        .collect();
    assert_eq!(texts, ["First turn"]);
}

#[test]
fn the_worker_keeps_serving_while_an_agent_takes_its_time_to_start_on_a_prompt() {
    let SlowWorker {
        worker,
        prompted,
        release,
        events,
        ..
    } = SlowWorker::start();
    let (receipt, _cancellation) = worker.prompt_sender().send(
        crate::PinnedAgent::new(agent("first", "session-1")),
        "Reviewer turn".into(),
    );
    prompted
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("the prompt is on its way");

    worker.send(Command::Thread(ThreadCommand::Load(UNIT.into())));

    assert!(loaded(&events).result.is_ok());
    release.send(()).unwrap();
    receipt.wait().unwrap();
}

#[test]
fn a_round_message_wakes_the_agent_with_its_round_and_the_question_it_was_asked_under() {
    let mut service = Service::start();
    let first = service.talk(
        "round-7",
        "Why a lock here?",
        Some(AskedUnder::Question {
            question: "q-lock".into(),
            version: 3,
            number: Some(2.into()),
        }),
    );
    let second = service.talk("round-7", "And in the design?", Some(AskedUnder::Design));

    let prompts = service.poll();
    let (_, access) = only_wakeup(&prompts);
    let prompt = &prompts[0].text;
    let thread = service.fetch(&access).unwrap().remove(0);
    assert_eq!(thread.round(), Some("round-7"));
    assert!(
        prompt.contains(&format!(
            "Round conversation: {}\nExplore round: round-7\n",
            thread.id.as_str()
        )),
        "{prompt}"
    );
    assert!(
        prompt.contains(&format!(
            "Message: {}\nQuestion: q-lock (version 3)\n",
            first.as_str()
        )),
        "{prompt}"
    );
    assert!(
        prompt.contains(&format!("Message: {}\nStage: design\n", second.as_str())),
        "{prompt}"
    );
    let asked =
        &prompt[prompt.find(first.as_str()).unwrap()..prompt.find(second.as_str()).unwrap()];
    assert!(
        asked.contains("Q2"),
        "the number the reviewer sees: {asked}"
    );

    service.answer(&access, &thread).unwrap();
    assert!(service.fetch(&access).unwrap().is_empty());
    service.talk("round-7", "One more", None);
    let prompts = service.poll();
    only_wakeup(&prompts);
    assert!(
        !prompts[0].text.contains(first.as_str()),
        "an answered message is not brought again"
    );
}

#[test]
fn a_wakeup_for_comments_on_code_names_no_round() {
    let mut service = Service::start();
    service.start_thread("Rename this");
    let prompts = service.poll();
    only_wakeup(&prompts);
    assert!(
        prompts[0]
            .text
            .ends_with(&format!("Logical review: {UNIT}")),
        "nothing about a round follows the review: {}",
        prompts[0].text
    );
}

#[test]
fn a_wakeup_that_does_not_reach_the_agent_names_the_comments_it_left_waiting() {
    let mut service = Service::start();
    service.agents.swallow_prompts(&pane("first"), true);
    service.talk("round-7", "Why a lock here?", None);
    only_wakeup(&service.poll());
    let book = service.state.books[&UNIT.into()].clone();
    let through = book.round_conversation("round-7").unwrap().messages[0].sequence();
    assert_eq!(
        service.wakeup_outcomes(),
        [
            None,
            Some(WakeupFailure {
                through,
                error: PromptError::NotStarted.to_string(),
            })
        ],
        "a wakeup on its way clears the failure, then its outcome reports it"
    );

    service.agents.swallow_prompts(&pane("first"), false);
    let thread = book.round_conversation("round-7").unwrap().id.clone();
    service.retry(&thread);
    only_wakeup(&service.poll());
    assert_eq!(service.wakeup_outcomes().last(), Some(&None));
}

#[test]
fn a_wakeup_without_an_agent_to_receive_it_names_the_comments_it_left_waiting() {
    let mut service = Service::start();
    service.agents.remove_agent(&pane("first"));
    service.talk("round-7", "Anyone there?", None);
    assert!(service.poll().is_empty());
    let outcomes = service.wakeup_outcomes();
    let [Some(failure)] = outcomes.as_slice() else {
        panic!("one failed wakeup, got {outcomes:?}");
    };
    let book = &service.state.books[&UNIT.into()];
    assert_eq!(
        failure.through,
        book.round_conversation("round-7").unwrap().messages[0].sequence()
    );
}

#[test]
fn each_new_message_in_a_round_s_conversation_is_published_once_with_its_round() {
    let mut service = Service::start();
    let first = service.talk("round-7", "Why a lock here?", None);
    // A reply typed in the pane names the conversation's thread only.
    let conversation = service.state.books[&ReviewUnit::from(UNIT)]
        .round_conversation("round-7")
        .unwrap()
        .id
        .clone();
    let reply = Post::reply(conversation, "And the timeout?".into());
    let second = reply.message().id.clone();
    service.thread(ThreadCommand::Post {
        review_unit: UNIT.into(),
        post: reply.clone(),
    });
    // A repeat of a post the threads hold, and a comment on code, are not new round messages.
    service.thread(ThreadCommand::Post {
        review_unit: UNIT.into(),
        post: reply,
    });
    service.start_thread("Rename this");

    assert_eq!(round_messages(&service, "round-7"), [first, second]);
    assert_eq!(service.round_messages.lock().unwrap().len(), 2);
}
