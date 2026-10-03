//! The page's routes. Every response comes from the latest stage the round's owner published.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{DefaultBodyLimit, FromRequestParts, Path, Query, Request, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use review_explore::{Alternative, Conclusion, Question};
use serde::{Deserialize, Serialize};

use crate::access::{Hosts, TokenCookie};
use crate::files::PageFiles;
use crate::round::{RoundFeed, RoundSnapshot, RoundStage, Rounds};

/// Scripts, styles and form posts only from the page itself, and no inline script or style.
/// The browser reports what the policy blocks to `/csp-report`.
const CONTENT_SECURITY_POLICY: &str = "default-src 'none'; script-src 'self'; \
     style-src 'self'; connect-src 'self'; img-src 'self'; form-action 'self'; \
     base-uri 'none'; frame-ancestors 'none'; report-uri /csp-report";

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
    /// A template failed to render.
    TemplateError(String),
}

/// The Explore page of a set of rounds, served by the caller on the page's own listener.
pub struct ExplorePage<R> {
    rounds: R,
    hosts: Hosts,
    files: PageFiles,
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
            log: Box::new(log),
        }
    }

    /// The page's routes, and the caller's own `routes`, all behind the page's [`Hosts`].
    pub fn into_router(self, routes: Router) -> Router {
        let page = Arc::new(self);
        Router::new()
            .route("/", get(index::<R>))
            .route("/status", get(status))
            .route("/assets/{name}", get(asset::<R>))
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

    /// The round whose token the request's cookie carries.
    fn admit(&self, headers: &HeaderMap) -> Result<RoundFeed, Refused> {
        TokenCookie::read(headers)
            .and_then(|token| self.rounds.find(token))
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

    fn show(&self, headers: &HeaderMap) -> Response {
        let round = match self.admit(headers) {
            Ok(round) => round.latest(),
            Err(refused) => return refused.into_response(),
        };
        let context = PageContext::new(&round, self.files.dev_version());
        match self.files.render("page.html", context) {
            Ok(html) => {
                let policy = (
                    header::CONTENT_SECURITY_POLICY,
                    HeaderValue::from_static(CONTENT_SECURITY_POLICY),
                );
                ([policy, NO_STORE], Html(html)).into_response()
            }
            Err(error) => {
                self.log(PageEvent::TemplateError(format!("{error:#}")));
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        }
    }

    fn refuse(&self, event: PageEvent) -> Refused {
        self.log(event);
        Refused
    }

    fn log(&self, event: PageEvent) {
        (self.log)(event);
    }
}

/// A request the page refused, once logged.
struct Refused;

impl IntoResponse for Refused {
    fn into_response(self) -> Response {
        StatusCode::FORBIDDEN.into_response()
    }
}

async fn admit_host<R: Rounds>(
    State(page): State<Arc<ExplorePage<R>>>,
    request: Request,
    next: Next,
) -> Response {
    match page.hosts.admit(request.headers()) {
        Ok(()) => next.run(request).await,
        Err(event) => page.refuse(event).into_response(),
    }
}

/// The round of a request whose cookie carries a round's token.
struct Admitted(RoundFeed);

impl<R: Rounds> FromRequestParts<Arc<ExplorePage<R>>> for Admitted {
    type Rejection = Refused;

    async fn from_request_parts(
        parts: &mut Parts,
        page: &Arc<ExplorePage<R>>,
    ) -> Result<Self, Refused> {
        page.admit(&parts.headers).map(Self)
    }
}

#[derive(Deserialize)]
struct TokenQuery {
    token: Option<String>,
}

/// The address the reviewer opens carries the token: the page trades it for a cookie, then
/// drops it from the address bar. Without a token in the address, shows the round.
async fn index<R: Rounds>(
    State(page): State<Arc<ExplorePage<R>>>,
    Query(query): Query<TokenQuery>,
    headers: HeaderMap,
) -> Response {
    match query.token {
        Some(token) => page.open(&token),
        None => page.show(&headers),
    }
}

/// What the template `page.html` receives.
#[derive(Serialize)]
struct PageContext<'a> {
    revision: u64,
    stage: Stage,
    question: Option<QuestionContext<'a>>,
    conclusion: Option<&'a Conclusion>,
    /// The count of file changes, in development only: the page reloads when it changes.
    dev_version: Option<u64>,
}

#[derive(Serialize)]
struct QuestionContext<'a> {
    number: usize,
    text: &'a str,
    /// The agent's alternatives, then None of the above.
    choices: Vec<&'a Alternative>,
}

/// The stage's name, as the template tests it: `no_round`, `working` (the page polls its status
/// only then), `question`, `interrupted` or `conclusion`.
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum Stage {
    NoRound,
    Working,
    Question,
    Interrupted,
    Conclusion,
}

impl<'a> PageContext<'a> {
    fn new(round: &'a RoundSnapshot, dev_version: Option<u64>) -> Self {
        let (stage, question, conclusion) = match &round.stage {
            RoundStage::NoRound => (Stage::NoRound, None, None),
            RoundStage::AgentWorking => (Stage::Working, None, None),
            RoundStage::Question { number, question } => (
                Stage::Question,
                Some(QuestionContext::new(*number, question)),
                None,
            ),
            RoundStage::Interrupted => (Stage::Interrupted, None, None),
            RoundStage::Conclusion(conclusion) => (Stage::Conclusion, None, Some(&**conclusion)),
        };
        Self {
            revision: round.revision,
            stage,
            question,
            conclusion,
            dev_version,
        }
    }
}

impl<'a> QuestionContext<'a> {
    fn new(number: usize, question: &'a Question) -> Self {
        Self {
            number,
            text: &question.text,
            choices: question.choices().collect(),
        }
    }
}

/// What the page's script polls while the agent works.
async fn status(Admitted(round): Admitted) -> Response {
    let revision = round.latest().revision;
    (
        [NO_STORE],
        Json(serde_json::json!({ "revision": revision })),
    )
        .into_response()
}

async fn asset<R: Rounds>(
    State(page): State<Arc<ExplorePage<R>>>,
    Path(name): Path<String>,
) -> Response {
    match page.files.asset(&name) {
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

/// Development only: answers once a template or an asset changes after `since`, or after a
/// while with the same count.
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
