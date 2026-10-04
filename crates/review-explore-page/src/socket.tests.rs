//! The page's socket, as a browser would use it: admitted like a page request, it sends the
//! round when it opens and at each change, carries the reviewer's actions to the round's owner,
//! and closes once its token opens no round.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use review_explore::{Question, RailStep, RoundOverview, Step, StepState, TabTitle};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::{Error, Message};

use crate::{
    Answered, CommandSender, ExplorePage, Hosts, ImplementationState, Interruption, LatestAnswer,
    PageCommand, PageFiles, PageImplementation, PageRound, PublishedRound, Recovery,
    RoundPublisher, RoundStage, Rounds, Token, Waiting,
};

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

const TOKEN: &str = "test-token";
const ROUND: &str = "round-1";

/// The round of the test's page, its owner's commands, and whether its token still opens it.
/// The owner takes each command as the review tool would, in a few lines: an answer or a Retry
/// puts the agent to work, a start starts, Stop waiting pauses the turn or drops the start, an
/// Implement sends the request, a quiz pick is saved.
#[derive(Clone)]
struct Owner {
    publisher: Arc<RoundPublisher>,
    commands: Arc<Mutex<Vec<String>>>,
    open: Arc<Mutex<bool>>,
    latest: Arc<Mutex<Option<LatestAnswer>>>,
    /// The round's overview, which most tests leave empty.
    overview: Arc<Mutex<RoundOverview>>,
}

impl Owner {
    fn new(stage: RoundStage) -> Self {
        let owner = Self {
            publisher: Arc::new(RoundPublisher::default()),
            commands: Arc::default(),
            open: Arc::new(Mutex::new(true)),
            latest: Arc::default(),
            overview: Arc::new(Mutex::new(RoundOverview {
                rail: Vec::new(),
                decisions: Vec::new(),
                earlier: Vec::new(),
                title: TabTitle::AgentWorking,
            })),
        };
        owner.publish(stage);
        owner
    }

    fn publish(&self, stage: RoundStage) {
        let latest = lock(&self.latest).clone();
        let overview = lock(&self.overview).clone();
        let round = PublishedRound {
            id: ROUND,
            design: None,
            cancellable: latest.as_ref(),
            earlier: false,
            overview: &overview,
        };
        self.publisher.publish(Some(round), stage);
    }

    /// The commands the page sent, by kind, in order.
    fn commands(&self) -> Vec<String> {
        lock(&self.commands).clone()
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
            PageCommand::Start { start, .. } => ("start", Some(RoundStage::Starting { start })),
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
        if let Some(next) = next {
            self.publish(next);
        }
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
            reply.send(Ok(()));
        });
        Some(PageRound::new(self.publisher.subscribe(), commands))
    }
}

fn working(request: &str) -> RoundStage {
    RoundStage::AgentWorking {
        request: request.into(),
    }
}

fn interrupted(request: &str, attempt: &str) -> RoundStage {
    RoundStage::Interrupted {
        request: Some(request.into()),
        attempt: Some(attempt.into()),
        interruption: Interruption::Stopped,
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

    let pick = json!({ "round": ROUND, "question": "q1", "version": 1, "choice": "discard" });
    request(
        &mut socket,
        json!({ "id": 1, "method": "pick", "params": pick }),
    )
    .await;
    let shown = next(&mut socket).await;
    let view = &shown["params"]["view"]["question"];
    assert_eq!(view["recommendation"], "shown_after_pick");
    assert_eq!(view["first_pick"], "Discard the draft");
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
async fn the_page_asks_whether_the_tool_is_there() {
    let address = serve(Owner::new(no_round())).await;
    let mut socket = Upgrade::of(address).connect(address).await.unwrap();
    next(&mut socket).await;

    request(&mut socket, json!({ "id": 9, "method": "ping" })).await;

    assert_eq!(next(&mut socket).await["id"], 9);
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
