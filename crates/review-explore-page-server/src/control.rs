//! What a test does in its own session: open it, then play its agent and the reviewer's pane.
//!
//! - `POST /test/sessions` opens a session whose agent works on its first question, and
//!   answers `{"token": "..."}`. The page of its round opens at `/?token=...`. Its review is a
//!   fixed one, which the start screen names.
//! - `POST /test/sessions/{token}/{step}` moves the session's round one step:
//!   - `question`: the agent posts its next question: the JSON `Question` of the request's
//!     body, or the fixed questions in turn when the body is empty; from the second question
//!     on, after a fixed response to the previous answer;
//!   - `answer`: the reviewer answers in the pane, and the agent works on its next turn;
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
//! - `POST /test/sessions/{token}/unreview-line`: the reviewer unmarks a line, and a round can
//!   start again.
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
//!   `"cancel-answer"`, `"reset"`, `"reply"`, `"cancel-implementation"`,
//!   `"resend-implementation"`.
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
        .route(
            "/test/sessions/{token}/unreview-line",
            post(|state, path| async move { block_starts(state, path, None) }),
        )
        .route("/test/sessions/{token}/actions", get(actions))
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

async fn open_session(State(sessions): State<Sessions>) -> Response {
    let token = Token::random();
    let body = serde_json::json!({ "token": token.to_string() });
    sessions.open(token, 0);
    Json(body).into_response()
}

async fn step(
    State(sessions): State<Sessions>,
    Path((token, step)): Path<(String, String)>,
    body: Bytes,
) -> Response {
    let Some(step) = Step::parse(&step) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let question = if body.is_empty() {
        None
    } else {
        match serde_json::from_slice::<Question>(&body) {
            Ok(question) => Some(question),
            Err(error) => {
                return (
                    StatusCode::BAD_REQUEST,
                    format!("invalid question: {error}"),
                )
                    .into_response();
            }
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
