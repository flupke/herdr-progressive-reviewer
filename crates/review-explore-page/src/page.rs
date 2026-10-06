//! The page's routes: the shell that loads the page's client, its assets, and its socket.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{DefaultBodyLimit, FromRequestParts, Path, Query, Request, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

use crate::access::{Hosts, TokenCookie};
use crate::blind::FirstPicks;
use crate::diagram;
use crate::files::PageFiles;
use crate::round::Rounds;

/// Scripts and styles only from the page itself, and no inline script; the page's requests go
/// only to itself, its socket included (named in full, `ws:`, or `wss:` through a tunnel: older
/// browsers do not count it to the page's own address as `'self'`). Inline styles are allowed for Mermaid, which writes them into
/// each diagram it draws: without them its boxes and labels are misplaced. The page posts no
/// form. The browser reports what the policy blocks to `/csp-report`.
fn content_security_policy(socket: &str) -> String {
    format!(
        "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
         connect-src 'self' {socket}; img-src 'self'; form-action 'none'; base-uri 'none'; \
         frame-ancestors 'none'; report-uri /csp-report"
    )
}

/// Every response is read again, never from the browser's cache.
const NO_STORE: (HeaderName, HeaderValue) =
    (header::CACHE_CONTROL, HeaderValue::from_static("no-store"));

/// The largest report of a policy violation the page reads.
const CSP_REPORT_LIMIT: usize = 16 * 1024;

/// How long a page in development waits for a file change before it asks again.
const DEV_CHANGES_WAIT: Duration = Duration::from_secs(25);

/// Something the page refused or could not do, for the server's log.
#[derive(Debug)]
pub enum PageEvent {
    /// A request for a host name the page does not answer.
    UnknownHost,
    /// A request that may change something, sent from another site's page.
    ForeignOrigin,
    /// A request with a missing token, or one that opens no round.
    WrongToken,
    /// The browser blocked something under the content security policy; the browser's report.
    CspViolation(String),
}

/// The Explore page of a set of rounds, served by the caller on the page's own listener.
pub struct ExplorePage<R> {
    rounds: R,
    hosts: Hosts,
    files: PageFiles,
    /// The reviewer's first picks of blind questions.
    pub(crate) picks: FirstPicks,
    /// Changes with each start of the tool, when the revisions start again.
    epoch: String,
    log: Box<dyn Fn(PageEvent) + Send + Sync>,
}

impl<R: Rounds> ExplorePage<R> {
    /// `log` receives what the page refuses or fails to do.
    pub fn new(
        rounds: R,
        hosts: Hosts,
        files: PageFiles,
        log: impl Fn(PageEvent) + Send + Sync + 'static,
    ) -> Self {
        Self {
            rounds,
            hosts,
            files,
            picks: FirstPicks::default(),
            epoch: uuid::Uuid::new_v4().simple().to_string(),
            log: Box::new(log),
        }
    }

    /// The page's routes, and the caller's own `routes`, all behind the page's [`Hosts`].
    pub fn into_router(self, routes: Router) -> Router {
        let page = Arc::new(self);
        let (script_path, script) = diagram::script_route::<R>();
        Router::new()
            .route("/", get(index::<R>))
            .route(&script_path, script)
            .route("/ws", get(crate::socket::upgrade::<R>))
            .route(
                "/token",
                get(|_: Admitted| async { StatusCode::NO_CONTENT }),
            )
            .route("/assets/{name}", get(asset::<R>))
            .route("/assets/client/{name}", get(client_asset::<R>))
            .route("/dev/changes", get(dev_changes::<R>))
            .route(
                "/csp-report",
                post(csp_report::<R>).layer(DefaultBodyLimit::max(CSP_REPORT_LIMIT)),
            )
            .route("/health", get(|| async { StatusCode::NO_CONTENT }))
            .route("/favicon.ico", get(|| async { StatusCode::NO_CONTENT }))
            .with_state(page.clone())
            .merge(routes)
            .layer(middleware::from_fn_with_state(page, admit_host::<R>))
    }

    /// The rounds the page shows.
    pub(crate) fn rounds(&self) -> &R {
        &self.rounds
    }

    /// The value that tells this start of the tool from the others.
    pub(crate) fn epoch(&self) -> &str {
        &self.epoch
    }

    /// Whether the request's cookie carries a token that opens a round.
    fn admits(&self, headers: &HeaderMap) -> Result<(), Refused> {
        TokenCookie::read(headers)
            .and_then(|token| self.rounds.find(token))
            .map(|_| ())
            .ok_or_else(|| self.refuse(PageEvent::WrongToken))
    }

    /// Trades the address's token for the cookie, then drops it from the address bar.
    fn open(&self, token: &str) -> Response {
        let Some(cookie) = self
            .rounds
            .find(token)
            .and_then(|_| TokenCookie::set(token))
        else {
            return self.refuse(PageEvent::WrongToken).into_response();
        };
        let mut response = Redirect::to("/").into_response();
        response.headers_mut().insert(header::SET_COOKIE, cookie);
        response
    }

    /// The shell that loads the page's client, which draws the round from its socket.
    fn show(&self, headers: &HeaderMap) -> Response {
        if let Err(refused) = self.admits(headers) {
            return refused.into_response();
        }
        // The host was checked against the page's own names already.
        let host = headers
            .get(header::HOST)
            .and_then(|host| host.to_str().ok())
            .unwrap_or_default();
        let socket = self.hosts.socket(host);
        let Ok(policy) = HeaderValue::from_str(&content_security_policy(&socket)) else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        let policy = (header::CONTENT_SECURITY_POLICY, policy);
        // A link in the agent's text does not tell another site the page's address on this
        // network.
        let referrer = (
            header::REFERRER_POLICY,
            HeaderValue::from_static("same-origin"),
        );
        let sniffing = (
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        );
        let html = self.files.shell();
        ([policy, NO_STORE, referrer, sniffing], Html(html)).into_response()
    }

    pub(crate) fn refuse(&self, event: PageEvent) -> Refused {
        self.log(event);
        Refused
    }

    fn log(&self, event: PageEvent) {
        (self.log)(event);
    }
}

/// A request the page refused, once logged.
pub(crate) struct Refused;

impl IntoResponse for Refused {
    fn into_response(self) -> Response {
        // A phone keeps the page of a round the reviewer has closed since: say what to do.
        (
            StatusCode::FORBIDDEN,
            [NO_STORE],
            "This address does not open an Explore round any more. Open the page again from \
             the pane: its QR code, or the Herdr action that opens it.",
        )
            .into_response()
    }
}

async fn admit_host<R: Rounds>(
    State(page): State<Arc<ExplorePage<R>>>,
    mut request: Request,
    next: Next,
) -> Response {
    match page.hosts.admit(request.headers()) {
        Ok(cookies) => {
            cookies.receive(request.headers_mut());
            let mut response = next.run(request).await;
            cookies.send(response.headers_mut());
            response
        }
        Err(event) => page.refuse(event).into_response(),
    }
}

/// A request whose cookie carries a round's token.
pub(crate) struct Admitted;

impl<R: Rounds> FromRequestParts<Arc<ExplorePage<R>>> for Admitted {
    type Rejection = Refused;

    async fn from_request_parts(
        parts: &mut Parts,
        page: &Arc<ExplorePage<R>>,
    ) -> Result<Self, Refused> {
        page.admits(&parts.headers).map(|()| Self)
    }
}

#[derive(Deserialize)]
struct PageQuery {
    token: Option<String>,
}

/// The address the reviewer opens carries the token: the page trades it for a cookie, then
/// drops it from the address bar. Without a token in the address, serves the page.
async fn index<R: Rounds>(
    State(page): State<Arc<ExplorePage<R>>>,
    Query(query): Query<PageQuery>,
    headers: HeaderMap,
) -> Response {
    match query.token {
        Some(token) => page.open(&token),
        None => page.show(&headers),
    }
}

async fn asset<R: Rounds>(
    State(page): State<Arc<ExplorePage<R>>>,
    Path(name): Path<String>,
) -> Response {
    serve_asset(&page.files, &name)
}

/// A module of the page's client.
async fn client_asset<R: Rounds>(
    State(page): State<Arc<ExplorePage<R>>>,
    Path(name): Path<String>,
) -> Response {
    serve_asset(&page.files, &format!("client/{name}"))
}

fn serve_asset(files: &PageFiles, name: &str) -> Response {
    match files.asset(name) {
        Some((content_type, body)) => (
            [
                (header::CONTENT_TYPE, HeaderValue::from_static(content_type)),
                NO_STORE,
            ],
            body,
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[derive(Deserialize)]
struct ChangesQuery {
    since: u64,
}

/// Development only: answers once a file of the page changes after `since`, or after a while
/// with the same count.
async fn dev_changes<R: Rounds>(
    State(page): State<Arc<ExplorePage<R>>>,
    _: Admitted,
    Query(query): Query<ChangesQuery>,
) -> Response {
    let Some(mut changes) = page.files.changes() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let _ = tokio::time::timeout(
        DEV_CHANGES_WAIT,
        changes.wait_for(|count| *count != query.since),
    )
    .await;
    let version = *changes.borrow();
    Json(serde_json::json!({ "version": version })).into_response()
}

async fn csp_report<R: Rounds>(
    State(page): State<Arc<ExplorePage<R>>>,
    body: String,
) -> StatusCode {
    page.log(PageEvent::CspViolation(body));
    StatusCode::NO_CONTENT
}
