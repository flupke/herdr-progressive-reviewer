//! The page's socket, as a browser would use it: admitted like a page request, it sends the
//! round when it opens and at each change, carries the reviewer's actions to the round's owner,
//! and closes once its token opens no round.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use review_explore::{Question, RailStep, RoundOverview, Step, StepState, TabTitle};
use review_explore_tally::{FileTally, MarkTally, MarkedLines, PendingLines, Tally};
use review_repository::repository::DiffStatistics;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::{Error, Message};

use review_threads::{MessageId, Post, ReviewThreads, ThreadCommand, WakeupFailure};
use review_types::ReviewUnit;

use crate::{
    Answered, CommandRefusal, CommandReply, CommandSender, ExplorePage, Hosts, ImplementationState,
    Interruption, LatestAnswer, PageCommand, PageConversation, PageFiles, PageImplementation,
    PageRound, PublishedRound, Recovery, RoundPublisher, RoundStage, Rounds, ThreadSender,
    ThreadsPublisher, Token, Waiting,
};

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

const TOKEN: &str = "test-token";
const ROUND: &str = "round-1";
const REVIEW: &str = "review-1";

/// The round of the test's page, its owner's commands, and whether its token still opens it.
/// The owner takes each command as the review tool would, in a few lines: an answer or a Retry
/// puts the agent to work, a start starts, Stop waiting pauses the turn or drops the start, an
/// Implement sends the request, a quiz pick is saved.
#[derive(Clone)]
struct Owner {
    publisher: Arc<RoundPublisher>,
    commands: Arc<Mutex<Vec<String>>>,
    /// Signalled each time a command arrives.
    command_arrived: Arc<tokio::sync::Notify>,
    open: Arc<Mutex<bool>>,
    latest: Arc<Mutex<Option<LatestAnswer>>>,
    /// The round's overview, which most tests leave empty.
    overview: Arc<Mutex<RoundOverview>>,
    /// The review's threads, which hold the round's conversation, as their owner keeps them.
    book: Arc<Mutex<ReviewThreads>>,
    threads: Arc<ThreadsPublisher>,
    /// The thread commands the page sent, by kind, in order.
    thread_commands: Arc<Mutex<Vec<String>>>,
    /// While set, the owner keeps its replies here and sends none, as an owner still at work.
    held: Arc<Mutex<Option<Vec<CommandReply>>>>,
}

impl Owner {
    fn new(stage: RoundStage) -> Self {
        let owner = Self {
            publisher: Arc::new(RoundPublisher::default()),
            commands: Arc::default(),
            command_arrived: Arc::default(),
            open: Arc::new(Mutex::new(true)),
            latest: Arc::default(),
            overview: Arc::new(Mutex::new(RoundOverview {
                rail: Vec::new(),
                decisions: Vec::new(),
                earlier: Vec::new(),
                title: TabTitle::AgentWorking,
            })),
            book: Arc::new(Mutex::new(ReviewThreads::new(REVIEW.into()))),
            threads: Arc::default(),
            thread_commands: Arc::default(),
            held: Arc::default(),
        };
        owner.publish(stage);
        owner
    }

    fn publish(&self, stage: RoundStage) {
        let latest = lock(&self.latest).clone();
        let overview = lock(&self.overview).clone();
        let review: ReviewUnit = REVIEW.into();
        let round = PublishedRound {
            id: ROUND,
            review_unit: &review,
            design: None,
            changed_files: 0,
            cancellable: latest.as_ref(),
            earlier: false,
            overview: &overview,
            earlier_citations: &[],
        };
        self.publisher.publish(Some(round), stage);
    }

    /// The commands the page sent, by kind, in order.
    fn commands(&self) -> Vec<String> {
        lock(&self.commands).clone()
    }

    /// Waits until the page sent a command.
    async fn first_command(&self) {
        while self.commands().is_empty() {
            self.command_arrived.notified().await;
        }
    }

    fn take(&self, command: PageCommand) {
        let stage = self.publisher.subscribe().stage();
        let (kind, next) = match command {
            PageCommand::Answer(answer) => {
                *lock(&self.latest) = Some(LatestAnswer {
                    id: "answer-1".into(),
                    choice: None,
                    comment: answer.input.text.clone(),
                    answered: Answered {
                        question: Some((answer.question, answer.version)),
                        option: answer.input.option,
                        in_reply_to: "turn-1".into(),
                    },
                });
                ("answer", Some(working("turn-2")))
            }
            PageCommand::Pick { .. } => ("pick", None),
            PageCommand::Start { start, .. } => (
                "start",
                Some(RoundStage::Starting {
                    start,
                    started_at_ms: None,
                }),
            ),
            PageCommand::Recover(Recovery::Stop(Waiting::Start(_))) => (
                "stop",
                Some(RoundStage::NoRound {
                    start: "start-2".into(),
                }),
            ),
            PageCommand::Recover(Recovery::Stop(Waiting::Turn(request))) => {
                ("stop", Some(interrupted(&request, "attempt-1")))
            }
            PageCommand::Recover(Recovery::Retry { request, .. }) => {
                ("retry", Some(working(&request)))
            }
            PageCommand::Implement(implement) => {
                let RoundStage::Conclusion {
                    request,
                    conclusion,
                    quiz,
                    response,
                    ..
                } = stage
                else {
                    panic!("no conclusion");
                };
                let implementation = PageImplementation {
                    delivery: "delivery-1".into(),
                    attempt: "attempt-1".into(),
                    text: implement.text,
                    state: ImplementationState::Sending,
                    sent_at_ms: None,
                };
                let next = RoundStage::Conclusion {
                    request,
                    conclusion,
                    implementation: Some(implementation),
                    quiz,
                    response,
                };
                ("implement", Some(next))
            }
            PageCommand::Quiz(response) => {
                let RoundStage::Conclusion {
                    request,
                    conclusion,
                    implementation,
                    mut quiz,
                    response: turn,
                } = stage
                else {
                    panic!("no conclusion");
                };
                quiz.answers
                    .record(&conclusion.quiz, response.response)
                    .unwrap();
                let next = RoundStage::Conclusion {
                    request,
                    conclusion,
                    implementation,
                    quiz,
                    response: turn,
                };
                ("quiz", Some(next))
            }
            _ => ("other", None),
        };
        lock(&self.commands).push(kind.to_owned());
        self.command_arrived.notify_one();
        if let Some(next) = next {
            self.publish(next);
        }
    }
}

impl Owner {
    /// Carries out a thread command the page sent, as the owner of the review threads would,
    /// then publishes the threads.
    fn take_thread_command(&self, command: ThreadCommand) -> Result<(), CommandRefusal> {
        let mut book = lock(&self.book);
        let (kind, result) = match command {
            ThreadCommand::Post { post, .. } => ("post", book.post(post).map(|_| ())),
            ThreadCommand::MarkRead {
                thread_id, through, ..
            } => ("mark-read", book.mark_read(&thread_id, through)),
            ThreadCommand::Retry { thread_id, .. } => ("retry", book.retry(&thread_id)),
            _ => ("other", Ok(())),
        };
        lock(&self.thread_commands).push(kind.to_owned());
        self.threads.loaded(book.clone());
        result.map_err(CommandRefusal::Failed)
    }

    fn thread_commands(&self) -> Vec<String> {
        lock(&self.thread_commands).clone()
    }

    /// The agent replies `text` to the reviewer's latest message of the round's conversation.
    fn agent_replies(&self, text: &str) {
        let mut book = lock(&self.book);
        let thread = book.round_conversation(ROUND).unwrap();
        let post = Post::answer(
            thread.id.clone(),
            MessageId::parse(&uuid::Uuid::new_v4().to_string()).unwrap(),
            text.into(),
            thread.last_comment().unwrap().id.clone(),
        );
        book.answer(post).unwrap();
        self.threads.loaded(book.clone());
    }

    /// The wakeup for the reviewer's pending messages did not reach the agent, for `error`.
    fn wakeup_fails(&self, error: &str) {
        let through = lock(&self.book).pending_comment_sequence().unwrap();
        let failure = WakeupFailure {
            through,
            error: error.into(),
        };
        self.threads.wakeup(REVIEW.into(), Some(failure));
    }
}

impl Rounds for Owner {
    fn find(&self, token: &str) -> Option<PageRound> {
        if !*lock(&self.open) || !Token::chosen(TOKEN.into()).unwrap().matches(token) {
            return None;
        }
        let owner = self.clone();
        let commands = CommandSender::new(move |command, reply| {
            owner.take(command);
            match lock(&owner.held).as_mut() {
                Some(held) => held.push(reply),
                None => reply.send(Ok(())),
            }
        });
        let owner = self.clone();
        let threads = ThreadSender::new(move |command, reply| {
            reply.send(owner.take_thread_command(command));
        });
        let conversation = PageConversation::new(self.threads.subscribe(), threads);
        Some(PageRound::new(self.publisher.subscribe(), commands).with_conversation(conversation))
    }
}

fn working(request: &str) -> RoundStage {
    RoundStage::AgentWorking {
        request: request.into(),
        sent_at_ms: None,
        answer: None,
    }
}

fn interrupted(request: &str, attempt: &str) -> RoundStage {
    RoundStage::Interrupted {
        request: Some(request.into()),
        attempt: Some(attempt.into()),
        interruption: Interruption::Stopped,
        answer: None,
    }
}

fn no_round() -> RoundStage {
    RoundStage::NoRound {
        start: "start-1".into(),
    }
}

/// A conclusion with a quiz of one item.
fn concluding() -> RoundStage {
    let conclusion = serde_json::from_value(json!({
        "summary": "Keep the draft.",
        "to_be_implemented": "Save the draft with the round.",
        "future_work": "",
        "quiz": [{
            "question": "What keeps the draft?", "answers": ["The round", "Nothing"], "correct": 0,
            "why": "The round saves it.", "proof": [], "level": "data model",
        }],
    }))
    .unwrap();
    RoundStage::Conclusion {
        request: "turn-9".into(),
        conclusion: Box::new(conclusion),
        implementation: None,
        quiz: crate::PageQuiz {
            proofs: vec![Arc::from([])],
            answers: review_explore::QuizAnswers::default(),
            takes_answers: true,
        },
        response: crate::TurnResponse::default(),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A question with two choices, whose Door is `door`.
fn question(id: &str, door: &str) -> Question {
    let assessments = (door != "none").then(|| {
        let lens = json!({ "summary": "Reason.", "evidence": [], "unknowns": ["None known."] });
        json!({ "door": door, "reversibility": lens, "blast_radius": lens })
    });
    serde_json::from_value(json!({
        "id": id,
        "version": 1,
        "topic": "drafts",
        "text": "Keep the draft?",
        "rationale": null,
        "visual": null,
        "alternatives": [
            { "id": "keep", "text": "Keep the draft", "outcome": "accepted", "recommendation": "It is cheap." },
            { "id": "discard", "text": "Discard the draft", "outcome": "needs_follow_up", "recommendation": null },
        ],
        "evidence": [],
        "assessments": assessments,
    }))
    .unwrap()
}

fn asking(question: Question) -> RoundStage {
    RoundStage::Question {
        number: 1,
        question: Box::new(question),
        citations: Arc::from([]),
        marks: crate::QuestionMarks::default(),
        response: crate::TurnResponse::default(),
        answer_cancelled: false,
    }
}

/// The page of `owner`, served on a free loopback port.
async fn serve(owner: Owner) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let page = ExplorePage::new(
        owner,
        Hosts::loopback(address.port()),
        PageFiles::embedded(),
        |_| {},
    );
    let app = page.into_router(axum::Router::new());
    // Without Nagle's algorithm, a reply is sent at once instead of after the delayed
    // acknowledgement of the view before it.
    let listener = axum::serve::ListenerExt::tap_io(listener, |stream| {
        let _ = stream.set_nodelay(true);
    });
    tokio::spawn(async move { axum::serve(listener, app).await });
    address
}

/// An upgrade request to the page at `address`, as the page's own script sends it.
struct Upgrade {
    host: String,
    origin: Option<String>,
    cookie: Option<String>,
}

impl Upgrade {
    fn of(address: SocketAddr) -> Self {
        Self {
            host: address.to_string(),
            origin: Some(format!("http://{address}")),
            cookie: Some(format!("explore_token_{}={TOKEN}", address.port())),
        }
    }

    async fn connect(self, address: SocketAddr) -> Result<Socket, Error> {
        let mut request = format!("ws://{address}/ws").into_client_request().unwrap();
        let headers = request.headers_mut();
        headers.insert("host", HeaderValue::from_str(&self.host).unwrap());
        for (name, value) in [("origin", self.origin), ("cookie", self.cookie)] {
            if let Some(value) = value {
                headers.insert(name, HeaderValue::from_str(&value).unwrap());
            }
        }
        tokio_tungstenite::connect_async(request)
            .await
            .map(|(socket, _)| socket)
    }
}

fn refused(result: Result<Socket, Error>) -> bool {
    matches!(result, Err(Error::Http(response)) if response.status() == 403)
}

/// The next message of the socket, as JSON; pings are skipped.
async fn next(socket: &mut Socket) -> Value {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .expect("a message within 5 s")
            .expect("the socket is open")
            .unwrap();
        let text = match message {
            Message::Text(text) => text,
            other => panic!("not text: {other:?}"),
        };
        let value: Value = serde_json::from_str(&text).unwrap();
        if value["method"] != "ping" {
            return value;
        }
    }
}

async fn request(socket: &mut Socket, request: Value) {
    socket
        .send(Message::Text(request.to_string().into()))
        .await
        .unwrap();
}

#[tokio::test]
async fn an_upgrade_without_a_token_that_opens_the_round_is_refused() {
    let address = serve(Owner::new(no_round())).await;

    let none = Upgrade {
        cookie: None,
        ..Upgrade::of(address)
    };
    assert!(refused(none.connect(address).await));
    let wrong = Upgrade {
        cookie: Some(format!("explore_token_{}=other", address.port())),
        ..Upgrade::of(address)
    };
    assert!(refused(wrong.connect(address).await));
    // The cookie of the page on another port of the address.
    let other_port = Upgrade {
        cookie: Some(format!("explore_token_1={TOKEN}")),
        ..Upgrade::of(address)
    };
    assert!(refused(other_port.connect(address).await));
    assert!(Upgrade::of(address).connect(address).await.is_ok());
}

#[tokio::test]
async fn an_upgrade_for_another_host_or_from_another_site_is_refused() {
    let address = serve(Owner::new(no_round())).await;

    let rebound = Upgrade {
        host: format!("attacker.example:{}", address.port()),
        ..Upgrade::of(address)
    };
    assert!(refused(rebound.connect(address).await));
    let foreign = Upgrade {
        origin: Some("http://attacker.example".into()),
        ..Upgrade::of(address)
    };
    assert!(refused(foreign.connect(address).await));
    let other_port = Upgrade {
        origin: Some("http://127.0.0.1:1".into()),
        ..Upgrade::of(address)
    };
    assert!(refused(other_port.connect(address).await));
}

#[tokio::test]
async fn the_socket_sends_the_round_when_it_opens_then_at_each_change() {
    let owner = Owner::new(working("turn-1"));
    let address = serve(owner.clone()).await;
    let mut socket = Upgrade::of(address).connect(address).await.unwrap();

    let first = next(&mut socket).await;
    assert_eq!(first["method"], "state");
    assert_eq!(first["params"]["view"]["question"], Value::Null);
    assert_eq!(first["params"]["view"]["reset"], ROUND);

    owner.publish(asking(question("q1", "two_way")));
    let second = next(&mut socket).await;
    assert_eq!(second["params"]["view"]["question"]["id"], "q1");
    assert_eq!(second["params"]["epoch"], first["params"]["epoch"]);
    let revision = |state: &Value| state["params"]["seq"]["revision"].as_u64().unwrap();
    assert!(revision(&second) > revision(&first));

    owner.publish(interrupted("turn-1", "attempt-1"));
    let third = next(&mut socket).await;
    assert_eq!(third["params"]["view"]["question"], Value::Null);
    let action = &third["params"]["view"]["cards"][0]["actions"][0];
    assert_eq!(action["method"], "retry");
    assert_eq!(action["fields"][0]["value"], "turn-1");
}

/// The rail of a round whose question 1 is done and question 2 current, the agent working on it
/// when `working`.
fn at_question_2(working: bool) -> RoundOverview {
    RoundOverview {
        rail: vec![
            RailStep {
                step: Step::Design,
                state: StepState::Done,
            },
            RailStep {
                step: Step::Question { number: 1 },
                state: StepState::Done,
            },
            RailStep {
                step: Step::Question { number: 2 },
                state: StepState::Current { working },
            },
        ],
        decisions: Vec::new(),
        earlier: Vec::new(),
        title: TabTitle::YourTurn { question: 2 },
    }
}

/// The reviewer's latest answer, to the question `question`.
fn answer_to(question: &str) -> LatestAnswer {
    LatestAnswer {
        id: format!("answer-{question}"),
        choice: Some("Keep the draft".into()),
        comment: String::new(),
        answered: Answered {
            question: Some((question.into(), 1)),
            option: Some("keep".into()),
            in_reply_to: "turn-1".into(),
        },
    }
}

#[tokio::test]
async fn the_previous_turn_names_the_question_the_latest_answer_answered_by_the_rail() {
    let owner = Owner::new(working("turn-1"));
    *lock(&owner.overview) = at_question_2(false);
    *lock(&owner.latest) = Some(answer_to("q1"));
    owner.publish(asking(question("q2", "two_way")));
    let address = serve(owner.clone()).await;
    let mut socket = Upgrade::of(address).connect(address).await.unwrap();
    // Question 2 asks: the latest answer answered the step before it.
    assert_eq!(next(&mut socket).await["params"]["view"]["answered"], 1);

    // The agent works on the answer to question 2: the latest answer answered the current step.
    *lock(&owner.overview) = at_question_2(true);
    *lock(&owner.latest) = Some(answer_to("q2"));
    owner.publish(working("turn-2"));
    assert_eq!(next(&mut socket).await["params"]["view"]["answered"], 2);
}

/// The reviewer's answer to `question`, sent with "Keep the draft" after a first pick of
/// "Discard the draft", and the marks it applied.
fn sent_answer(question: Question) -> crate::SentAnswer {
    let kept = question.alternatives[0].clone();
    crate::SentAnswer {
        question: Some(crate::AnsweredQuestion {
            question: Box::new(question),
            citations: Arc::from([]),
            picked_blind: true,
        }),
        kept: review_explore::KeptAnswer::new(Some(&kept), "Keep it.", Some("discard")),
        marked: review_explore::MarkCounts {
            reviewed_lines: 12,
            not_relevant_lines: 3,
            ..Default::default()
        },
    }
}

#[tokio::test]
async fn a_turn_that_carries_the_answer_shows_it_beside_its_card_with_its_question() {
    let owner = Owner::new(no_round());
    *lock(&owner.overview) = at_question_2(true);
    owner.publish(RoundStage::AgentWorking {
        request: "turn-2".into(),
        sent_at_ms: Some(1_000),
        answer: Some(Box::new(sent_answer(question("q2", "one_way")))),
    });
    let address = serve(owner.clone()).await;
    let mut socket = Upgrade::of(address).connect(address).await.unwrap();

    // The turn's card sits beside the answer, not above the stage.
    let view = next(&mut socket).await["params"]["view"].clone();
    assert_eq!(view["cards"], json!([]));
    let sent = &view["sent"];
    assert_eq!(sent["card"]["since"]["ms"], 1_000);
    assert_eq!(sent["card"]["actions"][0]["method"], "stop");
    assert_eq!(sent["card"]["actions"][0]["fields"][0]["value"], "turn-2");
    assert_eq!(sent["number"], 2);
    assert_eq!(sent["question"]["id"], "q2");
    assert_eq!(sent["question"]["number"], 2);
    assert_eq!(sent["question"]["answerable"], false);
    assert_eq!(sent["question"]["recommendation"], "shown_after_pick");
    assert_eq!(sent["answer"]["choice"], "Keep the draft");
    assert_eq!(sent["answer"]["comment"], "Keep it.");
    assert_eq!(
        sent["answer"]["tags"],
        json!(["changed_after_first_pick", "as_recommended"])
    );
    assert_eq!(sent["marked"]["parts"], json!(["15 lines reviewed"]));

    // The turn did not go through: the same answer, with Retry of the turn.
    owner.publish(RoundStage::Interrupted {
        request: Some("turn-2".into()),
        attempt: Some("attempt-2".into()),
        interruption: Interruption::NotStarted,
        answer: Some(Box::new(sent_answer(question("q2", "one_way")))),
    });
    let view = next(&mut socket).await["params"]["view"].clone();
    assert_eq!(view["cards"], json!([]));
    assert_eq!(view["sent"]["card"]["actions"][0]["method"], "retry");
    assert_eq!(view["sent"]["question"]["id"], "q2");

    // A turn that carries no answer, as the kickoff, shows its card above the stage.
    owner.publish(working("turn-3"));
    let view = next(&mut socket).await["params"]["view"].clone();
    assert_eq!(view["sent"], Value::Null);
    assert_eq!(view["cards"][0]["actions"][0]["method"], "stop");
}

#[tokio::test]
async fn the_socket_sends_the_round_again_when_only_its_review_marks_change() {
    let owner = Owner::new(working("turn-1"));
    let mut socket = open(&owner).await;
    let tally = |marked: u64| {
        MarkTally::of_files(
            vec![FileTally {
                path: "src/queue.rs".into(),
                tally: Tally::new(
                    DiffStatistics {
                        lines_added: 4,
                        lines_removed: 0,
                    },
                    MarkedLines {
                        by_hand: marked,
                        ..MarkedLines::default()
                    },
                    PendingLines::default(),
                    0,
                ),
                whole: None,
                cited: false,
            }],
            false,
        )
    };

    owner.publisher.tally(tally(1));
    let view = &next(&mut socket).await["params"]["view"];
    assert_eq!(
        view["tally"]["change"]["share"],
        json!({ "marked": 1, "changed": 4, "percent": 25 })
    );
    assert_eq!(view["tally"]["files"][0]["path"], "src/queue.rs");

    // The same marks again send nothing; the next view is the next change.
    owner.publisher.tally(tally(1));
    owner.publisher.tally(tally(2));
    let view = &next(&mut socket).await["params"]["view"];
    assert_eq!(view["tally"]["change"]["share"]["marked"], 2);

    // A stage and its marks come in one view.
    let overview = lock(&owner.overview).clone();
    let review: ReviewUnit = REVIEW.into();
    let round = PublishedRound {
        id: ROUND,
        review_unit: &review,
        design: None,
        cancellable: None,
        earlier: false,
        overview: &overview,
        changed_files: 1,
        earlier_citations: &[],
    };
    owner
        .publisher
        .publish_counted(Some(round), asking(question("q1", "two_way")), tally(3));
    let view = &next(&mut socket).await["params"]["view"];
    assert_eq!(view["question"]["id"], "q1");
    assert_eq!(view["tally"]["change"]["share"]["marked"], 3);
}

#[tokio::test]
async fn a_clarified_question_keeps_the_number_of_its_step_on_the_rail() {
    let owner = Owner::new(working("turn-1"));
    *lock(&owner.overview) = RoundOverview {
        rail: vec![
            RailStep {
                step: Step::Design,
                state: StepState::Done,
            },
            RailStep {
                step: Step::Question { number: 1 },
                state: StepState::Done,
            },
            RailStep {
                step: Step::Question { number: 2 },
                state: StepState::Current { working: false },
            },
        ],
        decisions: Vec::new(),
        earlier: Vec::new(),
        title: TabTitle::YourTurn { question: 2 },
    };
    // The round's third version of a question: a clarification of question 2.
    let RoundStage::Question {
        question,
        citations,
        marks,
        response,
        answer_cancelled,
        ..
    } = asking(question("q2", "two_way"))
    else {
        unreachable!()
    };
    owner.publish(RoundStage::Question {
        number: 3,
        question,
        citations,
        marks,
        response,
        answer_cancelled,
    });
    let address = serve(owner.clone()).await;
    let mut socket = Upgrade::of(address).connect(address).await.unwrap();

    let view = &next(&mut socket).await["params"]["view"];
    assert_eq!(view["question"]["number"], 2);
    assert_eq!(
        view["rail"][2]["step"],
        json!({ "kind": "question", "number": 2 })
    );
    assert_eq!(view["title"], json!({ "kind": "your_turn", "question": 2 }));
}

#[tokio::test]
async fn a_new_socket_gets_the_current_round_and_nothing_older() {
    let owner = Owner::new(working("turn-1"));
    let address = serve(owner.clone()).await;
    owner.publish(asking(question("q1", "two_way")));
    owner.publish(asking(question("q2", "two_way")));

    let mut socket = Upgrade::of(address).connect(address).await.unwrap();

    let state = next(&mut socket).await;
    assert_eq!(state["params"]["view"]["question"]["id"], "q2");
}

#[tokio::test]
async fn a_socket_whose_token_no_longer_opens_the_round_is_closed() {
    let owner = Owner::new(no_round());
    let address = serve(owner.clone()).await;
    let mut socket = Upgrade::of(address).connect(address).await.unwrap();
    next(&mut socket).await;

    *lock(&owner.open) = false;
    owner.publish(RoundStage::Starting {
        start: "start-1".into(),
        started_at_ms: None,
    });

    let closed = tokio::time::timeout(Duration::from_secs(5), socket.next())
        .await
        .unwrap();
    let Some(Ok(Message::Close(Some(frame)))) = closed else {
        panic!("the socket is not closed: {closed:?}");
    };
    assert_eq!(u16::from(frame.code), super::TOKEN_ENDED);
}

#[tokio::test]
async fn a_request_after_the_token_ended_is_not_carried_out_and_closes_the_socket() {
    let owner = Owner::new(asking(question("q1", "two_way")));
    let address = serve(owner.clone()).await;
    let mut socket = Upgrade::of(address).connect(address).await.unwrap();
    next(&mut socket).await;

    *lock(&owner.open) = false;
    let answer =
        json!({ "round": ROUND, "question": "q1", "version": 1, "choice": "keep", "comment": "" });
    request(
        &mut socket,
        json!({ "id": 7, "method": "answer", "params": answer }),
    )
    .await;

    let closed = tokio::time::timeout(Duration::from_secs(5), socket.next())
        .await
        .unwrap();
    let Some(Ok(Message::Close(Some(frame)))) = closed else {
        panic!("the socket is not closed: {closed:?}");
    };
    assert_eq!(u16::from(frame.code), super::TOKEN_ENDED);
    assert!(owner.commands().is_empty());
}

#[tokio::test]
async fn a_request_after_the_token_ended_is_refused_while_another_waits_for_its_reply() {
    let owner = Owner::new(asking(question("q1", "two_way")));
    let address = serve(owner.clone()).await;
    let mut socket = Upgrade::of(address).connect(address).await.unwrap();
    next(&mut socket).await;
    *lock(&owner.held) = Some(Vec::new());
    let answer =
        json!({ "round": ROUND, "question": "q1", "version": 1, "choice": "keep", "comment": "" });
    request(
        &mut socket,
        json!({ "id": 6, "method": "answer", "params": answer }),
    )
    .await;
    tokio::time::timeout(Duration::from_secs(5), owner.first_command())
        .await
        .expect("the answer reaches the owner within 5 s");

    *lock(&owner.open) = false;
    request(
        &mut socket,
        json!({ "id": 7, "method": "answer", "params": answer }),
    )
    .await;

    let reply = loop {
        let message = next(&mut socket).await;
        if message["id"] == 7 {
            break message;
        }
    };
    assert_eq!(reply["error"]["code"], super::RpcError::STALE);
    assert!(
        reply["error"]["data"].is_null(),
        "refused by the socket, not by the action's own check: {reply}"
    );
    assert_eq!(owner.commands(), ["answer"]);
}

#[tokio::test]
async fn an_action_reaches_the_owner_and_its_reply_follows_the_round_it_changed() {
    let owner = Owner::new(asking(question("q1", "two_way")));
    let address = serve(owner.clone()).await;
    let mut socket = Upgrade::of(address).connect(address).await.unwrap();
    next(&mut socket).await;

    let answer = json!({ "round": ROUND, "question": "q1", "version": 1, "choice": "keep", "comment": "Keep it." });
    request(
        &mut socket,
        json!({ "id": 7, "method": "answer", "params": answer }),
    )
    .await;

    let state = next(&mut socket).await;
    assert_eq!(state["method"], "state");
    assert_eq!(state["params"]["view"]["question"], Value::Null);
    let reply = next(&mut socket).await;
    assert_eq!(
        reply,
        json!({ "id": 7, "result": { "applied": true, "reopen": null } })
    );
    assert_eq!(owner.commands(), ["answer"]);
}

#[tokio::test]
async fn an_action_on_what_the_round_moved_past_is_refused_with_its_notice() {
    let owner = Owner::new(asking(question("q2", "two_way")));
    let address = serve(owner.clone()).await;
    let mut socket = Upgrade::of(address).connect(address).await.unwrap();
    next(&mut socket).await;

    let answer =
        json!({ "round": ROUND, "question": "q1", "version": 1, "choice": "keep", "comment": "" });
    request(
        &mut socket,
        json!({ "id": 3, "method": "answer", "params": answer }),
    )
    .await;

    let reply = next(&mut socket).await;
    assert_eq!(reply["id"], 3);
    assert_eq!(reply["error"]["code"], 409);
    assert_eq!(reply["error"]["data"]["id"], "notice");
    assert_eq!(reply["error"]["data"]["role"], "alert");
    assert!(owner.commands().is_empty());

    // A refused answer names the question by the number the page showed.
    let answer = json!({ "round": ROUND, "question": "q1", "version": 1, "choice": "keep", "comment": "", "number": 1 });
    request(
        &mut socket,
        json!({ "id": 4, "method": "answer", "params": answer }),
    )
    .await;
    let reply = next(&mut socket).await;
    let title = reply["error"]["data"]["title"].as_str().unwrap();
    assert!(title.contains("Question 1"), "{title}");
}

#[tokio::test]
async fn a_request_the_page_could_not_have_sent_is_refused() {
    let address = serve(Owner::new(no_round())).await;
    let mut socket = Upgrade::of(address).connect(address).await.unwrap();
    next(&mut socket).await;

    request(
        &mut socket,
        json!({ "id": 4, "method": "launch", "params": {} }),
    )
    .await;

    let reply = next(&mut socket).await;
    assert_eq!(reply["id"], 4);
    assert_eq!(reply["error"]["code"], -32600);
    assert_eq!(reply["error"]["data"], Value::Null);
}

#[tokio::test]
async fn a_blind_questions_first_pick_shows_its_recommendation_and_is_kept_once() {
    let owner = Owner::new(asking(question("q1", "one_way")));
    let address = serve(owner.clone()).await;
    let mut socket = Upgrade::of(address).connect(address).await.unwrap();
    let hidden = next(&mut socket).await;
    assert_eq!(
        hidden["params"]["view"]["question"]["recommendation"],
        "hidden_until_pick"
    );
    // Nothing the page holds before the first pick tells the recommended choice: not its reason,
    // nor a mark on the choice.
    assert!(!hidden.to_string().contains("It is cheap."));
    let choices = &hidden["params"]["view"]["question"]["choices"];
    assert!(
        choices
            .as_array()
            .unwrap()
            .iter()
            .all(|choice| choice["recommendation"].is_null() && choice["checked"] == false)
    );

    let pick = json!({ "round": ROUND, "question": "q1", "version": 1, "choice": "discard" });
    request(
        &mut socket,
        json!({ "id": 1, "method": "pick", "params": pick }),
    )
    .await;
    let shown = next(&mut socket).await;
    let view = &shown["params"]["view"]["question"];
    assert_eq!(view["recommendation"], "shown_after_pick");
    let picked: Vec<&Value> = view["choices"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|choice| choice["checked"] == true)
        .map(|choice| &choice["id"])
        .collect();
    assert_eq!(picked, [&json!("discard")]);
    let recommended = view["choices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|choice| choice["id"] == "keep")
        .unwrap();
    assert_eq!(recommended["recommendation"], "It is cheap.");
    assert_eq!(next(&mut socket).await["result"]["applied"], true);

    let again = json!({ "round": ROUND, "question": "q1", "version": 1, "choice": "keep" });
    request(
        &mut socket,
        json!({ "id": 2, "method": "pick", "params": again }),
    )
    .await;
    assert_eq!(next(&mut socket).await["result"]["applied"], false);
    assert_eq!(owner.commands(), ["pick"]);
}

#[tokio::test]
async fn a_question_names_its_door_the_lead_of_each_section_and_the_share_its_answer_adds() {
    let mut stage = asking(question("q1", "one_way"));
    if let RoundStage::Question { marks, .. } = &mut stage {
        marks.reviewed = vec![lines("src/drafts.rs", 1, 12)];
    }
    let owner = Owner::new(stage);
    owner.publisher.tally(waiting(gain(52, 64, 135)));
    let address = serve(owner).await;
    let mut socket = Upgrade::of(address).connect(address).await.unwrap();

    let view = next(&mut socket).await;
    let question = &view["params"]["view"]["question"];
    assert_eq!(question["door"], "one_way");
    let sections = question["sections"].as_array().unwrap();
    assert_eq!(sections[0]["title"], "Door");
    assert!(
        sections[0]["lead_html"]
            .as_str()
            .unwrap()
            .contains("Reason.")
    );
    assert!(
        sections[0]["details_html"]
            .as_str()
            .unwrap()
            .contains("None known.")
    );
    assert_eq!(
        question["marks"]["summary"],
        json!({ "verb": "Answering marks", "parts": ["12 lines reviewed"] })
    );
    assert_eq!(question["gain"], json!({ "before": 38, "after": 47 }));
}

#[tokio::test]
async fn a_question_whose_answer_marks_nothing_shows_no_gain() {
    let owner = Owner::new(asking(question("q1", "two_way")));
    owner.publisher.tally(waiting(gain(52, 52, 135)));
    let address = serve(owner).await;
    let mut socket = Upgrade::of(address).connect(address).await.unwrap();

    let view = next(&mut socket).await;
    assert_eq!(view["params"]["view"]["question"]["marks"], Value::Null);
    assert_eq!(view["params"]["view"]["question"]["gain"], Value::Null);
}

/// The tally of a change whose waiting question's answer does as `gain` says.
fn waiting(gain: review_explore_tally::Gain) -> MarkTally {
    MarkTally {
        gain: Some(gain),
        ..MarkTally::default()
    }
}

/// What answering marks: the share goes from `before` to `after` marked lines of `changed`.
fn gain(before: u64, after: u64, changed: u64) -> review_explore_tally::Gain {
    let pending = review_explore_tally::PendingLines {
        reviewed: after - before,
        not_relevant: 0,
    };
    review_explore_tally::Gain::new(pending, 0, review_explore_tally::Share::of(before, changed))
}

/// Lines of `path` after the change.
fn lines(path: &str, first_line: u32, last_line: u32) -> review_explore::CodeLocation {
    serde_json::from_value(json!({
        "path": path,
        "side": "new",
        "lines": { "first_line": first_line, "last_line": last_line },
    }))
    .unwrap()
}

#[tokio::test]
async fn the_page_asks_whether_the_tool_is_there() {
    let address = serve(Owner::new(no_round())).await;
    let mut socket = Upgrade::of(address).connect(address).await.unwrap();
    next(&mut socket).await;

    request(&mut socket, json!({ "id": 9, "method": "ping" })).await;

    let reply = next(&mut socket).await;
    assert_eq!(reply["id"], 9);
    assert_eq!(reply["result"]["applied"], false);
    assert!(reply["error"].is_null());
}

/// Opens a socket to the page of `owner`, past its first view.
async fn open(owner: &Owner) -> Socket {
    let address = serve(owner.clone()).await;
    let mut socket = Upgrade::of(address).connect(address).await.unwrap();
    next(&mut socket).await;
    socket
}

/// Sends `method` with `params` twice, and returns the two replies.
async fn twice(socket: &mut Socket, method: &str, params: &Value) -> (Value, Value) {
    let mut replies = Vec::new();
    for id in [1, 2] {
        request(
            socket,
            json!({ "id": id, "method": method, "params": params }),
        )
        .await;
        replies.push(reply(socket, id).await);
    }
    let second = replies.pop().unwrap();
    (replies.pop().unwrap(), second)
}

/// The reply to the request `id`, past the views before it.
async fn reply(socket: &mut Socket, id: u64) -> Value {
    loop {
        let message = next(socket).await;
        if message["id"] == id {
            return message;
        }
    }
}

fn applied(reply: &Value) -> Option<bool> {
    reply["result"]["applied"].as_bool()
}

#[tokio::test]
async fn a_repeated_answer_is_applied_once() {
    let owner = Owner::new(asking(question("q1", "two_way")));
    let mut socket = open(&owner).await;

    let answer = json!({ "round": ROUND, "question": "q1", "version": 1, "choice": "keep", "comment": "Keep it." });
    let (first, second) = twice(&mut socket, "answer", &answer).await;

    assert_eq!(
        (applied(&first), applied(&second)),
        (Some(true), Some(false))
    );
    assert_eq!(owner.commands(), ["answer"]);
    // Another answer to the same question is no repeat.
    let other = json!({ "round": ROUND, "question": "q1", "version": 1, "choice": "discard", "comment": "Keep it." });
    request(
        &mut socket,
        json!({ "id": 3, "method": "answer", "params": other }),
    )
    .await;
    assert_eq!(reply(&mut socket, 3).await["error"]["code"], 409);
}

#[tokio::test]
async fn a_repeated_start_starts_one_round_and_a_late_one_none() {
    let owner = Owner::new(no_round());
    let mut socket = open(&owner).await;

    let start = json!({ "challenger": false, "start": "start-1" });
    let (first, second) = twice(&mut socket, "start", &start).await;
    assert_eq!(
        (applied(&first), applied(&second)),
        (Some(true), Some(false))
    );

    // Stop waiting drops the start; the start screen offers the next one.
    request(
        &mut socket,
        json!({ "id": 3, "method": "stop", "params": { "start": "start-1" } }),
    )
    .await;
    assert_eq!(applied(&reply(&mut socket, 3).await), Some(true));
    request(
        &mut socket,
        json!({ "id": 4, "method": "start", "params": start }),
    )
    .await;
    assert_eq!(reply(&mut socket, 4).await["error"]["code"], 409);
    assert_eq!(owner.commands(), ["start", "stop"]);
}

#[tokio::test]
async fn a_repeated_stop_waiting_stops_once() {
    let owner = Owner::new(working("turn-1"));
    let mut socket = open(&owner).await;

    let (first, second) = twice(&mut socket, "stop", &json!({ "request": "turn-1" })).await;

    assert_eq!(
        (applied(&first), applied(&second)),
        (Some(true), Some(false))
    );
    assert_eq!(owner.commands(), ["stop"]);
}

#[tokio::test]
async fn a_repeated_retry_sends_the_turn_once_and_a_late_one_never() {
    let owner = Owner::new(interrupted("turn-1", "attempt-1"));
    let mut socket = open(&owner).await;

    let retry = json!({ "request": "turn-1", "attempt": "attempt-1" });
    let (first, second) = twice(&mut socket, "retry", &retry).await;
    assert_eq!(
        (applied(&first), applied(&second)),
        (Some(true), Some(false))
    );

    // The retried prompt fails at once: its next attempt waits for another Retry.
    owner.publish(interrupted("turn-1", "attempt-2"));
    request(
        &mut socket,
        json!({ "id": 3, "method": "retry", "params": retry }),
    )
    .await;
    assert_eq!(reply(&mut socket, 3).await["error"]["code"], 409);
    assert_eq!(owner.commands(), ["retry"]);
}

#[tokio::test]
async fn a_repeated_implement_sends_one_request() {
    let owner = Owner::new(concluding());
    let mut socket = open(&owner).await;

    let implement = json!({ "conclusion": "turn-9", "replaces": null, "text": "Save the draft." });
    let (first, second) = twice(&mut socket, "implement", &implement).await;

    assert_eq!(
        (applied(&first), applied(&second)),
        (Some(true), Some(false))
    );
    assert_eq!(owner.commands(), ["implement"]);
}

#[tokio::test]
async fn a_repeated_quiz_answer_is_saved_once() {
    let owner = Owner::new(concluding());
    let mut socket = open(&owner).await;

    let pick = json!({ "conclusion": "turn-9", "item": 0, "answer": 1 });
    let (first, second) = twice(&mut socket, "quiz", &pick).await;

    assert_eq!(
        (applied(&first), applied(&second)),
        (Some(true), Some(false))
    );
    assert_eq!(owner.commands(), ["quiz"]);
}

/// A message of the reviewer's, for the round's conversation, with the identity `id`.
fn message(id: &str, round: &str) -> Value {
    json!({
        "round": round,
        "id": id,
        "text": "Why keep the draft?",
        "asked_under": { "stage": "question", "question": "q1", "version": 1 },
        "quote": "Keep the draft",
    })
}

const MESSAGE_ID: &str = "4f0c3d1e-58a2-4b6e-9d61-0a9f0f7c2b11";

/// The round's conversation in the latest view the socket sent, past the reply `id`.
async fn conversation_after(socket: &mut Socket, id: u64) -> (Value, Value) {
    let mut conversation = Value::Null;
    loop {
        let message = next(socket).await;
        if message["method"] == "state" {
            conversation = message["params"]["view"]["conversation"].clone();
        }
        if message["id"] == id {
            return (message, conversation);
        }
    }
}

#[tokio::test]
async fn a_message_joins_the_rounds_conversation_and_leaves_its_question_waiting() {
    let mut overview = at_question_2(false);
    overview.rail[2].state = StepState::Later;
    overview.rail[1].state = StepState::Current { working: false };
    let owner = Owner::new(working("turn-1"));
    *lock(&owner.overview) = overview;
    owner.publish(asking(question("q1", "two_way")));
    let mut socket = open(&owner).await;

    request(
        &mut socket,
        json!({ "id": 1, "method": "send-message", "params": message(MESSAGE_ID, ROUND) }),
    )
    .await;
    let (reply, conversation) = conversation_after(&mut socket, 1).await;

    assert_eq!(applied(&reply), Some(true));
    // A message is a thread's comment, never a command of the round's owner.
    assert_eq!(owner.commands(), Vec::<String>::new());
    assert_eq!(owner.thread_commands(), ["post"]);
    let thread = lock(&owner.book).round_conversation(ROUND).unwrap().clone();
    let posted = &thread.messages[0];
    assert_eq!(posted.id.as_str(), MESSAGE_ID);
    assert_eq!(
        posted.asked_under,
        Some(review_threads::AskedUnder::Question {
            question: "q1".into(),
            version: 1,
            number: Some(1.into()),
        }),
        "the message carries the number the page shows its question by"
    );
    assert_eq!(posted.quote.as_deref(), Some("Keep the draft"));
    let shown = &conversation["messages"][0];
    assert_eq!(shown["author"], "reviewer");
    assert_eq!(shown["place"], "Q1");
    assert_eq!(shown["quote"], "Keep the draft");
    assert_eq!(shown["delivery"]["state"], "waiting");
    assert_eq!(conversation["round"], ROUND);
}

#[tokio::test]
async fn a_message_names_a_question_asked_again_by_the_step_of_its_version() {
    let versioned = |id: &str, version: u32| Question {
        version,
        ..question(id, "two_way")
    };
    let earlier = |number, question| review_explore::EarlierQuestion {
        number,
        question,
        answer: None,
        recorded: review_explore::AgentRecord {
            interpretation: None,
            reply: None,
        },
        marks: Vec::new(),
    };
    let mut overview = at_question_2(false);
    overview.rail[2].state = StepState::Done;
    overview.rail.push(RailStep {
        step: Step::Question { number: 3 },
        state: StepState::Current { working: false },
    });
    overview.earlier = vec![
        earlier(1, versioned("cache", 1)),
        earlier(2, versioned("lock", 1)),
    ];
    let owner = Owner::new(working("turn-1"));
    *lock(&owner.overview) = overview;
    owner.publish(asking(versioned("cache", 2)));
    let mut socket = open(&owner).await;

    for (id, (version, message_id)) in [
        (1, "4f0c3d1e-58a2-4b6e-9d61-0a9f0f7c2b12"),
        (2, "4f0c3d1e-58a2-4b6e-9d61-0a9f0f7c2b13"),
    ]
    .into_iter()
    .enumerate()
    {
        let mut params = message(message_id, ROUND);
        params["asked_under"] =
            json!({ "stage": "question", "question": "cache", "version": version });
        request(
            &mut socket,
            json!({ "id": id, "method": "send-message", "params": params }),
        )
        .await;
        conversation_after(&mut socket, id as u64).await;
    }

    let numbers: Vec<_> = lock(&owner.book)
        .round_conversation(ROUND)
        .unwrap()
        .messages
        .iter()
        .map(|message| match &message.asked_under {
            Some(review_threads::AskedUnder::Question { number, .. }) => {
                number.map(|number| number.to_string())
            }
            other => panic!("asked under a question: {other:?}"),
        })
        .collect();
    assert_eq!(numbers, [Some("Q1".to_owned()), Some("Q3".to_owned())]);
}

#[tokio::test]
async fn a_repeated_message_is_posted_once() {
    let owner = Owner::new(asking(question("q1", "two_way")));
    let mut socket = open(&owner).await;

    let (first, second) = twice(&mut socket, "send-message", &message(MESSAGE_ID, ROUND)).await;

    assert_eq!(
        (applied(&first), applied(&second)),
        (Some(true), Some(false))
    );
    assert_eq!(owner.thread_commands(), ["post"]);
    let book = lock(&owner.book);
    assert_eq!(book.round_conversation(ROUND).unwrap().messages.len(), 1);
}

#[tokio::test]
async fn a_message_for_a_round_the_page_no_longer_shows_is_refused() {
    let owner = Owner::new(asking(question("q1", "two_way")));
    let mut socket = open(&owner).await;

    request(
        &mut socket,
        json!({ "id": 1, "method": "send-message", "params": message(MESSAGE_ID, "round-0") }),
    )
    .await;

    assert_eq!(reply(&mut socket, 1).await["error"]["code"], 409);
    assert_eq!(owner.thread_commands(), Vec::<String>::new());
}

#[tokio::test]
async fn the_agents_reply_shows_unread_until_the_chat_marks_it_read() {
    let owner = Owner::new(asking(question("q1", "two_way")));
    let mut socket = open(&owner).await;
    request(
        &mut socket,
        json!({ "id": 1, "method": "send-message", "params": message(MESSAGE_ID, ROUND) }),
    )
    .await;
    reply(&mut socket, 1).await;

    owner.agent_replies("Because it is cheap.");
    let view = next(&mut socket).await;
    let conversation = &view["params"]["view"]["conversation"];
    assert_eq!(conversation["unread"], 1);
    assert_eq!(conversation["messages"][0]["delivery"]["state"], "answered");
    assert_eq!(conversation["messages"][1]["author"], "agent");
    assert_eq!(conversation["messages"][1]["unread"], true);

    let through = conversation["read_through"].clone();
    request(
        &mut socket,
        json!({ "id": 2, "method": "read-messages", "params": { "round": ROUND, "through": through } }),
    )
    .await;
    let (_, read) = conversation_after(&mut socket, 2).await;
    assert_eq!(read["unread"], 0);
    assert_eq!(owner.thread_commands(), ["post", "mark-read"]);
}

#[tokio::test]
async fn a_message_that_did_not_reach_the_agent_offers_retry_once() {
    let owner = Owner::new(asking(question("q1", "two_way")));
    let mut socket = open(&owner).await;
    request(
        &mut socket,
        json!({ "id": 1, "method": "send-message", "params": message(MESSAGE_ID, ROUND) }),
    )
    .await;
    reply(&mut socket, 1).await;

    owner.wakeup_fails("No agent is focused");
    let view = next(&mut socket).await;
    let conversation = &view["params"]["view"]["conversation"];
    assert_eq!(
        conversation["messages"][0]["delivery"]["state"],
        "not_delivered"
    );
    let action = &conversation["card"]["actions"][0];
    assert_eq!(action["method"], "retry-messages");
    assert_eq!(action["fields"][0]["value"], ROUND);

    let retry = json!({ "round": ROUND });
    request(
        &mut socket,
        json!({ "id": 2, "method": "retry-messages", "params": retry }),
    )
    .await;
    assert_eq!(applied(&reply(&mut socket, 2).await), Some(true));
    // Once the agent replied, no message waits: a late Retry wakes nothing.
    owner.agent_replies("Sorry, here it is.");
    request(
        &mut socket,
        json!({ "id": 3, "method": "retry-messages", "params": retry }),
    )
    .await;
    assert_eq!(reply(&mut socket, 3).await["error"]["code"], 409);
    assert_eq!(owner.thread_commands(), ["post", "retry"]);
}

/// The conclusion of [`concluding`], whose implementation request the pane sent and is in
/// `state`.
fn implemented(state: ImplementationState) -> RoundStage {
    let RoundStage::Conclusion {
        request,
        conclusion,
        quiz,
        response,
        ..
    } = concluding()
    else {
        unreachable!("a conclusion");
    };
    RoundStage::Conclusion {
        request,
        conclusion,
        implementation: Some(PageImplementation {
            delivery: "delivery-1".into(),
            attempt: "attempt-1".into(),
            text: "Save the draft with the round.".into(),
            state,
            sent_at_ms: None,
        }),
        quiz,
        response,
    }
}

/// The status card of the notice that refused the request `id`, which reached no owner.
async fn refusal(socket: &mut Socket, id: u64, method: &str, params: Value) -> Value {
    request(
        socket,
        json!({ "id": id, "method": method, "params": params }),
    )
    .await;
    let reply = reply(socket, id).await;
    assert_eq!(reply["error"]["code"], 409, "{method}: {reply}");
    assert_eq!(reply["error"]["data"]["role"], "alert", "{method}");
    reply["error"]["data"].clone()
}

#[tokio::test]
async fn each_action_the_round_moved_past_is_refused_with_a_notice_that_says_what_moved() {
    // A round started in the pane while the page showed the start screen.
    let owner = Owner::new(working("turn-1"));
    let mut socket = open(&owner).await;
    let start = json!({ "challenger": false, "start": "start-1" });
    let card = refusal(&mut socket, 1, "start", start).await;
    assert_eq!(card["title"], "A round was started meanwhile");
    // A Retry of a turn that is no longer interrupted.
    let retry = json!({ "request": "turn-0", "attempt": "attempt-1" });
    let card = refusal(&mut socket, 2, "retry", retry).await;
    assert_eq!(card["title"], "The turn is no longer interrupted");
    assert_eq!(owner.commands(), Vec::<String>::new());

    // A first pick of a blind question the pane answered while the page showed it.
    let owner = Owner::new(asking(question("q2", "one_way")));
    let mut socket = open(&owner).await;
    let pick =
        json!({ "round": ROUND, "question": "q1", "version": 1, "choice": "discard", "number": 1 });
    let card = refusal(&mut socket, 1, "pick", pick).await;
    assert_eq!(card["title"], "Question 1 was already answered");
    let reason = card["reason"].as_str().unwrap();
    assert!(reason.contains("pick"), "{reason}");
    assert_eq!(owner.commands(), Vec::<String>::new());

    // An Implement of a conclusion whose request the pane sent.
    let owner = Owner::new(implemented(ImplementationState::Sent));
    let mut socket = open(&owner).await;
    let implement = json!({ "conclusion": "turn-9", "replaces": null, "text": "Another list." });
    let card = refusal(&mut socket, 1, "implement", implement).await;
    assert_eq!(
        card["title"],
        "This conclusion no longer waits for a request"
    );
    assert_eq!(owner.commands(), Vec::<String>::new());
}

#[tokio::test]
async fn a_comment_and_a_list_written_on_the_page_keep_the_line_breaks_of_the_pane() {
    let owner = Owner::new(asking(question("q1", "two_way")));
    let mut socket = open(&owner).await;
    let answer = json!({ "round": ROUND, "question": "q1", "version": 1, "choice": "keep", "comment": "Keep it.\r\nThen log it." });
    request(
        &mut socket,
        json!({ "id": 1, "method": "answer", "params": answer }),
    )
    .await;
    assert_eq!(applied(&reply(&mut socket, 1).await), Some(true));
    let saved = lock(&owner.latest)
        .as_ref()
        .map(|answer| answer.comment.clone());
    assert_eq!(saved.as_deref(), Some("Keep it.\nThen log it."));

    owner.publish(concluding());
    let implement =
        json!({ "conclusion": "turn-9", "replaces": null, "text": "Save it.\r\nTest it." });
    request(
        &mut socket,
        json!({ "id": 2, "method": "implement", "params": implement }),
    )
    .await;
    assert_eq!(applied(&reply(&mut socket, 2).await), Some(true));
    let stage = owner.publisher.subscribe().stage();
    let sent = stage.implementation().map(|request| request.text.clone());
    assert_eq!(sent.as_deref(), Some("Save it.\nTest it."));
}

#[tokio::test]
async fn a_blind_question_asked_again_after_cancel_answer_shows_its_recommendation_and_keeps_no_pick()
 {
    let RoundStage::Question {
        number,
        question,
        citations,
        marks,
        response,
        ..
    } = asking(question("q1", "one_way"))
    else {
        unreachable!("a question");
    };
    let owner = Owner::new(RoundStage::Question {
        number,
        question,
        citations,
        marks,
        response,
        answer_cancelled: true,
    });
    let address = serve(owner.clone()).await;
    let mut socket = Upgrade::of(address).connect(address).await.unwrap();

    let view = next(&mut socket).await;
    let question = &view["params"]["view"]["question"];
    assert_eq!(question["recommendation"], "shown");
    let recommended = question["choices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|choice| choice["id"] == "keep")
        .unwrap();
    assert_eq!(recommended["recommendation"], "It is cheap.");

    let pick = json!({ "round": ROUND, "question": "q1", "version": 1, "choice": "discard" });
    request(
        &mut socket,
        json!({ "id": 1, "method": "pick", "params": pick }),
    )
    .await;
    assert_eq!(applied(&reply(&mut socket, 1).await), Some(false));
    assert_eq!(owner.commands(), Vec::<String>::new());
}

/// The IDs and roles of the status cards of the latest view the socket sent.
fn cards(view: &Value) -> Vec<(&str, &str)> {
    view["params"]["view"]["cards"]
        .as_array()
        .unwrap()
        .iter()
        .map(|card| (card["id"].as_str().unwrap(), card["role"].as_str().unwrap()))
        .collect()
}

#[tokio::test]
async fn a_start_that_failed_because_nothing_is_left_to_review_shows_only_that_once() {
    let owner = Owner::new(no_round());
    owner
        .publisher
        .block_starts(Some(review_explore::StartBlock::NothingToReview));
    let address = serve(owner.clone()).await;
    let mut socket = Upgrade::of(address).connect(address).await.unwrap();
    let blocked = [("start-block", "status")];
    assert_eq!(cards(&next(&mut socket).await), blocked);

    owner.publish(RoundStage::StartFailed {
        failure: review_explore::StartBlock::NothingToReview.reason().into(),
        start: "start-1".into(),
    });
    assert_eq!(cards(&next(&mut socket).await), blocked);

    // Another failure says why, beside the block.
    owner.publish(RoundStage::StartFailed {
        failure: "Repository comparison is not ready".into(),
        start: "start-1".into(),
    });
    assert_eq!(
        cards(&next(&mut socket).await),
        [("start-failure", "alert"), ("start-block", "status")]
    );
}

#[tokio::test]
async fn a_request_the_agent_may_have_received_is_sent_again_only_in_place_of_it() {
    let owner = Owner::new(implemented(ImplementationState::Unknown));
    let address = serve(owner.clone()).await;
    let mut socket = Upgrade::of(address).connect(address).await.unwrap();

    // The page offers no Implement, and one action that names the request it replaces.
    let view = next(&mut socket).await;
    let conclusion = &view["params"]["view"]["conclusion"];
    assert_eq!(conclusion["list"]["kind"], "request");
    assert_eq!(conclusion["list"]["edit"], false);
    let actions = conclusion["implementation_card"]["actions"]
        .as_array()
        .unwrap();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0]["method"], "implement");
    assert_eq!(actions[0]["tier"], "secondary");
    let fields: Vec<(&str, &str)> = actions[0]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|field| {
            (
                field["name"].as_str().unwrap(),
                field["value"].as_str().unwrap(),
            )
        })
        .collect();
    assert!(fields.contains(&("replaces", "delivery-1")), "{fields:?}");

    let anew = json!({ "conclusion": "turn-9", "replaces": null, "text": "Save the draft with the round." });
    refusal(&mut socket, 1, "implement", anew).await;
    assert_eq!(owner.commands(), Vec::<String>::new());
    let instead = json!({ "conclusion": "turn-9", "replaces": "delivery-1", "text": "Save the draft with the round." });
    request(
        &mut socket,
        json!({ "id": 2, "method": "implement", "params": instead }),
    )
    .await;
    assert_eq!(applied(&reply(&mut socket, 2).await), Some(true));
    assert_eq!(owner.commands(), ["implement"]);
}
