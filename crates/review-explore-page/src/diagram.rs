//! The diagrams of the agent's Markdown: the page draws each fenced `mermaid` block in the
//! browser with Mermaid, which the page serves itself, and reports a diagram Mermaid cannot
//! parse to the round's owner through its socket (`assets/client/diagrams.js`).

use std::sync::Arc;

use axum::http::{HeaderMap, HeaderValue, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{MethodRouter, get};

use crate::Rounds;
use crate::page::ExplorePage;

/// The address of Mermaid's script. It names the version, so the browser may keep the file.
pub(crate) fn script_path() -> String {
    format!("/assets/{}", mermaid_js::FILE_NAME)
}

/// The address of Mermaid's script, and its route.
pub(crate) fn script_route<R: Rounds>() -> (String, MethodRouter<Arc<ExplorePage<R>>>) {
    (script_path(), get(script))
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
