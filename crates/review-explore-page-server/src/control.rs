//! What a test does in its own session: open it, then play its agent.
//!
//! - `POST /test/sessions` opens a session whose agent works on its first question, and
//!   answers `{"token": "..."}`. The page of its round opens at `/?token=...`.
//! - `POST /test/sessions/{token}/question`: the session's agent posts the fixed question.
//!
//! These routes exist only in the standalone server. They sit behind the page's host and origin
//! checks, but need no token.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use review_explore_page::{RoundStage, Token};

use crate::sessions::Sessions;

pub(crate) fn router(sessions: Sessions) -> Router {
    Router::new()
        .route("/test/sessions", post(open_session))
        .route("/test/sessions/{token}/question", post(ask_question))
        .with_state(sessions)
}

async fn open_session(State(sessions): State<Sessions>) -> Response {
    let token = Token::random();
    let body = serde_json::json!({ "token": token.to_string() });
    sessions.open(token, RoundStage::AgentWorking);
    Json(body).into_response()
}

async fn ask_question(State(sessions): State<Sessions>, Path(token): Path<String>) -> StatusCode {
    if sessions.ask_question(&token) {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}
