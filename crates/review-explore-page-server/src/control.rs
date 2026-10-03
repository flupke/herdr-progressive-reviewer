//! What a test does in its own session: open it, then play its agent and the reviewer's pane.
//!
//! - `POST /test/sessions` opens a session whose agent works on its first question, and
//!   answers `{"token": "..."}`. The page of its round opens at `/?token=...`.
//! - `POST /test/sessions/{token}/{step}` moves the session's round one step:
//!   - `question`: the agent posts its next question: the JSON `Question` of the request's
//!     body, or the fixed questions in turn when the body is empty;
//!   - `answer`: the reviewer answers in the pane, and the agent works on its next turn;
//!   - `fail`: the prompt of the agent's next turn could not be delivered;
//!   - `cancel`: the reviewer cancels the latest answer in the pane, and its question waits
//!     again;
//!   - `interrupt`: the agent stops before its next turn;
//!   - `conclude`: the agent concludes the round;
//!   - `reset`: the reviewer resets the round, and no round is running;
//!   - `kickoff`: the tool sent the kickoff of the round the reviewer started from the page,
//!     and the agent works on its first turn;
//!   - `fail-start`: that round could not start, and no round is running.
//! - `GET /test/sessions/{token}/answers` lists the answers the reviewer sent from the page,
//!   in order: `[{"question", "version", "choice", "comment"}]`, with `"first_pick"` when
//!   the question hid its recommendation until the reviewer's first pick.
//! - `GET /test/sessions/{token}/diagram-errors` lists the diagram errors the session's page
//!   reported, each once: `[{"question", "version", "source", "message"}]`.
//! - `GET /test/sessions/{token}/starts` lists the rounds the reviewer started from the page,
//!   in order: `[{"challenger"}]`.
//!
//! These routes exist only in the standalone server. They sit behind the page's host and origin
//! checks, but need no token.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use review_explore::Question;
use review_explore_page::Token;

use crate::sessions::{Sessions, Step};

pub(crate) fn router(sessions: Sessions) -> Router {
    Router::new()
        .route("/test/sessions", post(open_session))
        .route("/test/sessions/{token}/answers", get(answers))
        .route("/test/sessions/{token}/diagram-errors", get(diagram_errors))
        .route("/test/sessions/{token}/starts", get(starts))
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
