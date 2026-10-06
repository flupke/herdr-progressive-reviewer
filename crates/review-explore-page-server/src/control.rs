//! What a test does in its own session: open it, then play its agent and the reviewer's pane.
//!
//! - `POST /test/sessions` opens a session whose agent works on its first question, and
//!   answers `{"token": "..."}`. The page of its round opens at `/?token=...`. Its review is a
//!   fixed one, which the start screen names. Its agent posts the server's data set, or the
//!   one a `{"data": "short" | "rich"}` body names.
//! - `POST /test/sessions/{token}/{step}` moves the session's round one step:
//!   - `question`: the agent posts its next question: the JSON `Question` of the request's
//!     body, or the fixed questions in turn when the body is empty; from the second question
//!     on, after a fixed response to the previous answer;
//!   - `question-prepared`: the same, as a turn that run-ahead prepared while the reviewer
//!     thought about the answer, which the page says;
//!   - `question-not-prepared`: the same, as a turn the agent took itself while run-ahead
//!     watched the question, because the fork of the answer's choice had not finished, which
//!     the page says;
//!   - `answer`: the reviewer answers in the pane, and the agent works on its next turn;
//!   - `answer-after-first-pick`: the reviewer answers with the recommended choice (the first
//!     one when none is) after a first pick of another choice, as on a blind question on the
//!     page, and the agent works on its next turn;
//!   - `fail`: the prompt the session sends could not be delivered: the conclusion's
//!     implementation request, while it sends one, or else the agent's next turn;
//!   - `not-started`: the agent did not start on the prompt the session sends: the
//!     conclusion's implementation request, while it sends one, or else its next turn;
//!   - `cancel`: the reviewer cancels the latest answer in the pane, and its question waits
//!     again, with its recommendation shown at once: the reviewer has seen it;
//!   - `reopen-unsent`: the reviewer reopens the review before the prompt the session sends
//!     went out: the conclusion's implementation request, while it sends one, is saved but not
//!     sent; or else the agent's next turn stopped;
//!   - `reopen-sending`: the reviewer reopens the review while that prompt was being
//!     delivered: whether the agent received it is unknown;
//!   - `interrupt`: the agent stops before its next turn;
//!   - `conclude`: the agent concludes the round, with an empty quiz, after a fixed recap of the
//!     previous answer;
//!   - `conclude-with-quiz`: the agent concludes the round with a quiz of two items;
//!   - `implement`: the reviewer implements the conclusion in the pane, and the agent receives
//!     the request;
//!   - `deliver`: the agent receives the implementation request the session sends;
//!   - `reset`: the reviewer resets the round, and no round is running;
//!   - `kickoff`: the tool sent the kickoff of the round the reviewer started from the page,
//!     and the agent works on its first turn;
//!   - `fail-start`: that round could not start, and no round is running; while nothing is
//!     left to review, it failed for that reason;
//!   - `earlier`: another reviewer saved a newer round of the review: the round stays where it
//!     is, and offers only Reset;
//!   - `fail-storage`: the review tool cannot save the reviewer's rounds any more.
//! - `POST /test/sessions/{token}/hold`: the page stops following the round, as a page whose
//!   socket does not get the tool's messages: it shows the round as it was until the reviewer
//!   acts on it; the action is then refused as stale, and the page follows the round again.
//! - `POST /test/sessions/{token}/restart`: the reviewer restarts: the page's socket closes, and
//!   the page cannot open another one (its upgrade gets 503) until
//!   `POST /test/sessions/{token}/back`. The round stays where it is, and may move meanwhile.
//! - `POST /test/sessions/{token}/review-everything`: the reviewer marks every changed line as
//!   reviewed, and no round can start: nothing is left to review. The round stays where it is.
//! - `POST /test/sessions/{token}/mark-by-hand`: the reviewer marks three more lines of the
//!   change's first file by hand, which the page's meter shows at once. The round stays where
//!   it is.
//! - `POST /test/sessions/{token}/agent-replies`: the agent replies, with the data set's fixed
//!   reply, to the reviewer's latest message in the conversation of the session's latest round.
//! - `POST /test/sessions/{token}/messages-not-delivered`: the wakeup for the reviewer's waiting
//!   messages did not reach the agent.
//! - `GET /test/sessions/{token}/messages` lists the messages the reviewer sent from the page's
//!   chat, in order: `[{"round", "text", "asked_under", "quote"}]`, `asked_under` as the review
//!   threads save it (`{"stage": "question", "question", "version"}`, `{"stage": "design"}`,
//!   `{"stage": "conclusion", "conclusion"}`) or `null`.
//! - `GET /test/sessions/{token}/answers` lists the answers the reviewer sent from the page,
//!   in order: `[{"question", "version", "choice", "comment"}]`, with `"first_pick"` when
//!   the question hid its recommendation until the reviewer's first pick.
//! - `GET /test/sessions/{token}/diagram-errors` lists the diagram errors the session's page
//!   reported, each once: `[{"question", "version", "source", "message"}]`.
//! - `GET /test/sessions/{token}/starts` lists the rounds the reviewer started from the page,
//!   in order: `[{"challenger"}]`.
//! - `GET /test/sessions/{token}/implementations` lists the lists to be implemented that the
//!   reviewer sent from the page, in order.
//! - `GET /test/sessions/{token}/actions` lists, by name and in order, the other actions the
//!   reviewer took on the page to recover or close the round: `"stop"`, `"retry"`,
//!   `"cancel-answer"`, `"reset"`, `"cancel-implementation"`, `"resend-implementation"`, and
//!   `"retry-messages"`, the chat's Retry of messages that did not reach the agent.
//! - `GET /test/sessions/{token}/quiz` gives what the reviewer answered of the conclusion's quiz,
//!   as the review tool saves it: `{"picks": [{"item", "answer", "correct"}], "skipped"}`, each
//!   field left out while empty, so `{}` when the round has no quiz.
//!
//! These routes exist only in the standalone server. They sit behind the page's host and origin
//! checks, but need no token.

use axum::body::Bytes;
use axum::extract::{Path, Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use review_explore::{Question, StartBlock};
use review_explore_page::Token;

use crate::round_data;
use crate::sessions::{PageChange, Sessions, Step};

pub(crate) fn router(sessions: Sessions) -> Router {
    Router::new()
        .route("/test/sessions", post(open_session))
        .route("/test/sessions/{token}/answers", get(answers))
        .route("/test/sessions/{token}/diagram-errors", get(diagram_errors))
        .route("/test/sessions/{token}/starts", get(starts))
        .route(
            "/test/sessions/{token}/implementations",
            get(implementations),
        )
        .route("/test/sessions/{token}/quiz", get(quiz))
        .route(
            "/test/sessions/{token}/review-everything",
            post(|state, path| async move {
                block_starts(state, path, Some(StartBlock::NothingToReview))
            }),
        )
        .route("/test/sessions/{token}/mark-by-hand", post(mark_by_hand))
        .route("/test/sessions/{token}/actions", get(actions))
        .route("/test/sessions/{token}/messages", get(messages))
        .route("/test/sessions/{token}/agent-replies", post(agent_replies))
        .route(
            "/test/sessions/{token}/messages-not-delivered",
            post(messages_not_delivered),
        )
        .route(
            "/test/sessions/{token}/hold",
            post(|state, path| async move { change_page(state, path, PageChange::Hold) }),
        )
        .route(
            "/test/sessions/{token}/restart",
            post(|state, path| async move { change_page(state, path, PageChange::Restart) }),
        )
        .route(
            "/test/sessions/{token}/back",
            post(|state, path| async move { change_page(state, path, PageChange::Back) }),
        )
        .route("/test/sessions/{token}/{step}", post(step))
        .with_state(sessions)
}

/// What a test may ask of the session it opens.
#[derive(Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionRequest {
    /// The data set its agent posts, by name.
    data: Option<String>,
}

async fn open_session(State(sessions): State<Sessions>, body: Bytes) -> Response {
    let request = match optional_json::<SessionRequest>(&body) {
        Ok(request) => request.unwrap_or_default(),
        Err(error) => {
            return (StatusCode::BAD_REQUEST, format!("invalid session: {error}")).into_response();
        }
    };
    let data = match request.data.as_deref().map(round_data::by_name) {
        None => None,
        Some(Some(data)) => Some(data),
        Some(None) => return (StatusCode::BAD_REQUEST, "unknown data set").into_response(),
    };
    let token = Token::random();
    let body = serde_json::json!({ "token": token.to_string() });
    sessions.open(token, 0, data);
    Json(body).into_response()
}

/// The JSON of a request's `body`, or `None` for an empty body; a body that does not parse is
/// refused with the reason.
fn optional_json<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<Option<T>, String> {
    if body.is_empty() {
        return Ok(None);
    }
    serde_json::from_slice(body)
        .map(Some)
        .map_err(|error| error.to_string())
}

async fn step(
    State(sessions): State<Sessions>,
    Path((token, step)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    let Some(step) = Step::parse(&step) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let question = match optional_json::<Question>(&body) {
        Ok(question) => question,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("invalid question: {error}"),
            )
                .into_response();
        }
    };
    if sessions.step(&token, step, question) {
        StatusCode::NO_CONTENT.into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

/// The review marks of the session behind `token` change, so that `block` says why no round can
/// start, or with `None` that one can.
fn block_starts(
    State(sessions): State<Sessions>,
    Path(token): Path<String>,
    block: Option<StartBlock>,
) -> StatusCode {
    if sessions.block_starts(&token, block) {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

/// The reviewer marks lines of the change of the session behind `token` by hand.
async fn mark_by_hand(State(sessions): State<Sessions>, Path(token): Path<String>) -> StatusCode {
    if sessions.mark_by_hand(&token) {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

/// The page of the session behind `token` changes as `change` says.
fn change_page(
    State(sessions): State<Sessions>,
    Path(token): Path<String>,
    change: PageChange,
) -> StatusCode {
    if sessions.change_page(&token, change) {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

async fn answers(State(sessions): State<Sessions>, Path(token): Path<String>) -> Response {
    match sessions.answers(&token) {
        Some(answers) => Json(answers).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn diagram_errors(State(sessions): State<Sessions>, Path(token): Path<String>) -> Response {
    match sessions.diagram_errors(&token) {
        Some(errors) => Json(errors).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn starts(State(sessions): State<Sessions>, Path(token): Path<String>) -> Response {
    match sessions.starts(&token) {
        Some(starts) => Json(starts).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn implementations(State(sessions): State<Sessions>, Path(token): Path<String>) -> Response {
    match sessions.implementations(&token) {
        Some(implementations) => Json(implementations).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn quiz(State(sessions): State<Sessions>, Path(token): Path<String>) -> Response {
    match sessions.quiz(&token) {
        Some(quiz) => Json(quiz).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// No content when the change was made, else not found.
fn found(changed: bool) -> StatusCode {
    if changed {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

async fn agent_replies(State(sessions): State<Sessions>, Path(token): Path<String>) -> StatusCode {
    found(sessions.agent_replies(&token))
}

async fn messages_not_delivered(
    State(sessions): State<Sessions>,
    Path(token): Path<String>,
) -> StatusCode {
    found(sessions.messages_not_delivered(&token))
}

async fn messages(State(sessions): State<Sessions>, Path(token): Path<String>) -> Response {
    match sessions.messages(&token) {
        Some(messages) => Json(messages).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn actions(State(sessions): State<Sessions>, Path(token): Path<String>) -> Response {
    match sessions.actions(&token) {
        Some(actions) => Json(actions).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Refuses the socket of a page whose reviewer is restarting, as a reviewer that is not there
/// yet: the page tries again later.
pub(crate) async fn away(
    State(sessions): State<Sessions>,
    request: Request,
    next: Next,
) -> Response {
    let restarting = request.uri().path() == "/ws"
        && request
            .headers()
            .get_all(header::COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .flat_map(|value| value.split(';'))
            .filter_map(|pair| pair.trim().split_once('='))
            .any(|(name, token)| name.starts_with("explore_token_") && sessions.away(token));
    if restarting {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    next.run(request).await
}
