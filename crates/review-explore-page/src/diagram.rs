//! The diagrams of the agent's Markdown: the page draws each fenced `mermaid` block in the
//! browser with Mermaid, which the page serves itself, and reports a diagram Mermaid cannot
//! parse to the round's owner (`assets/diagrams.js`).

use std::sync::Arc;

use axum::Json;
use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{MethodRouter, get, post};
use review_explore::DiagramError;
use serde::Serialize;

use crate::page::{Admitted, ExplorePage};
use crate::{PageCommand, Rounds};

/// The largest report of a diagram error the page reads: the diagram's source and Mermaid's
/// message.
const REPORT_LIMIT: usize = 64 * 1024;

/// What the templates need to draw diagrams.
#[derive(Serialize)]
pub(crate) struct Diagrams {
    /// The address of Mermaid's script.
    script: String,
    /// The language of a fenced block that holds a diagram.
    fence: &'static str,
}

impl Diagrams {
    pub(crate) fn new() -> Self {
        Self {
            script: script_path(),
            fence: mermaid_js::FENCE,
        }
    }
}

/// The address of Mermaid's script. It names the version, so the browser may keep the file.
fn script_path() -> String {
    format!("/assets/{}", mermaid_js::FILE_NAME)
}

/// The address of Mermaid's script, and its route.
pub(crate) fn script_route<R: Rounds>() -> (String, MethodRouter<Arc<ExplorePage<R>>>) {
    (script_path(), get(script))
}

/// The route the page reports a diagram error to.
pub(crate) fn report_route<R: Rounds>() -> MethodRouter<Arc<ExplorePage<R>>> {
    post(report).layer(DefaultBodyLimit::max(REPORT_LIMIT))
}

/// Mermaid's script, gzipped unless the browser does not accept it. Its address names its
/// version, so the browser keeps it.
async fn script(headers: HeaderMap) -> Response {
    let cache = (
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=31536000, immutable"),
    );
    let content_type = (
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/javascript"),
    );
    let vary = (header::VARY, HeaderValue::from_static("accept-encoding"));
    if accepts_gzip(&headers) {
        let encoding = (header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
        ([content_type, cache, vary, encoding], mermaid_js::GZIPPED).into_response()
    } else {
        ([content_type, cache, vary], mermaid_js::script()).into_response()
    }
}

fn accepts_gzip(headers: &HeaderMap) -> bool {
    headers
        .get_all(header::ACCEPT_ENCODING)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .filter_map(|coding| coding.split(';').next())
        .any(|coding| coding.trim().eq_ignore_ascii_case("gzip"))
}

/// A diagram of the question the page showed that Mermaid could not parse: the round's owner
/// saves the error with the question. The page shows the error whatever the owner replies.
async fn report(Admitted(round): Admitted, Json(error): Json<DiagramError>) -> StatusCode {
    match round.commands.send(PageCommand::DiagramFailed(error)).await {
        Ok(()) => StatusCode::NO_CONTENT,
        // The round moved on, or the owner could not save the error.
        Err(_) => StatusCode::CONFLICT,
    }
}
