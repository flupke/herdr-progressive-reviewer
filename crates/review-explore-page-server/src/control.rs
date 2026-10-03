//! What a test does in its own session: open it, then play its agent and the reviewer's pane.
//!
//! - `POST /test/sessions` opens a session whose agent works on its first question, and
//!   answers `{"token": "..."}`. The page of its round opens at `/?token=...`.
//! - `POST /test/sessions/{token}/{step}` moves the session's round one step:
//!   - `question`: the agent posts its next question, the fixed questions in turn;
//!   - `answer`: the reviewer answers in the pane, and the agent works on its next turn;
//!   - `interrupt`: the agent stops before its next turn;
//!   - `conclude`: the agent concludes the round;
//!   - `reset`: the reviewer resets the round, and no round is running.
//!
//! These routes exist only in the standalone server. They sit behind the page's host and origin
//! checks, but need no token.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use review_explore_page::Token;

use crate::sessions::{Sessions, Step};

pub(crate) fn router(sessions: Sessions) -> Router {
    Router::new()
        .route("/test/sessions", post(open_session))
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
) -> StatusCode {
    let Some(step) = Step::parse(&step) else {
        return StatusCode::NOT_FOUND;
    };
    if sessions.step(&token, step) {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}
