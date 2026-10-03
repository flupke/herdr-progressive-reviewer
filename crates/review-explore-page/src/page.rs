//! The page's routes. Every response comes from the latest stage the round's owner published.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{DefaultBodyLimit, Form, FromRequestParts, Path, Query, Request, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use review_explore::{
    Alternative, AnswerInput, Assessments, Conclusion, MarkTense, Question, QuestionSection,
};
use review_explore_citations::Citation;
use serde::{Deserialize, Serialize};

use crate::access::{Hosts, TokenCookie};
use crate::citation::CitationContext;
use crate::command::{PageAnswer, PageCommand};
use crate::files::PageFiles;
use crate::notice::Notice;
use crate::round::{PageRound, QuestionMarks, RoundSnapshot, RoundStage, Rounds};

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
            .route("/answer", post(answer))
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
    fn admit(&self, headers: &HeaderMap) -> Result<PageRound, Refused> {
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

    /// Shows the round, with the notice of a post that did not go through, once.
    fn show(&self, headers: &HeaderMap) -> Response {
        let round = match self.admit(headers) {
            Ok(round) => round.stages.latest(),
            Err(refused) => return refused.into_response(),
        };
        let notice = Notice::read(headers);
        let context = PageContext::new(&round, notice.as_ref(), self.files.dev_version());
        match self.files.render("page.html", context) {
            Ok(html) => {
                let policy = (
                    header::CONTENT_SECURITY_POLICY,
                    HeaderValue::from_static(CONTENT_SECURITY_POLICY),
                );
                let mut response = ([policy, NO_STORE], Html(html)).into_response();
                if notice.is_some() {
                    response
                        .headers_mut()
                        .insert(header::SET_COOKIE, Notice::clear());
                }
                response
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
        // A phone keeps the page of a round the reviewer has closed since: say what to do.
        (
            StatusCode::FORBIDDEN,
            [NO_STORE],
            "This address does not open an Explore round any more. Open the page again from \
             the reviewer: its QR code, or its Herdr action.",
        )
            .into_response()
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

/// The round of a request whose cookie carries a round's token. Every route that reads or
/// changes a round requires it.
struct Admitted(PageRound);

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

/// The reviewer's answer, as the question's form posts it.
#[derive(Deserialize)]
struct AnswerForm {
    question: String,
    version: u32,
    /// The picked choice's ID; absent when the reviewer picked none.
    choice: Option<String>,
    #[serde(default)]
    comment: String,
}

/// Hands the reviewer's answer to the round's owner, unless the page showed a question that
/// no longer waits for an answer, then shows the page again with what became of it.
async fn answer(Admitted(round): Admitted, Form(form): Form<AnswerForm>) -> Response {
    let sent = if round.stages.stage().asks(&form.question, form.version) {
        let answer = PageAnswer {
            question: form.question,
            version: form.version,
            input: AnswerInput {
                option: form.choice,
                text: form.comment,
                in_reply_to: None,
            },
        };
        round.commands.send(PageCommand::Answer(answer)).await
    } else {
        Err(Notice::Stale)
    };
    let mut response = Redirect::to("/").into_response();
    if let Err(notice) = sent {
        response
            .headers_mut()
            .insert(header::SET_COOKIE, notice.cookie());
    }
    response
}

/// What the template `page.html` receives. The templates turn the agent's Markdown into HTML
/// with the filter `markdown` (see [`PageFiles`]).
#[derive(Serialize)]
struct PageContext<'a> {
    revision: u64,
    stage: Stage,
    question: Option<QuestionContext<'a>>,
    conclusion: Option<&'a Conclusion>,
    /// Why the turn the agent no longer works on failed, when its prompt failed.
    failure: Option<&'a str>,
    /// Why the reviewer's latest post did not go through.
    notice: Option<&'a Notice>,
    /// The count of file changes, in development only: the page reloads when it changes.
    dev_version: Option<u64>,
}

#[derive(Serialize)]
struct QuestionContext<'a> {
    number: usize,
    id: &'a str,
    version: u32,
    text: &'a str,
    /// The Context section, in Markdown; empty when the question has none.
    context: String,
    /// The Door and Blast radius sections, folded away until the reviewer opens them.
    sections: Vec<QuestionSection>,
    /// The agent's alternatives, then None of the above.
    choices: Vec<&'a Alternative>,
    /// The question's citations, most decisive first.
    citations: Vec<CitationContext<'a>>,
    /// The lines an answer marks, `None` when it marks none.
    marks: Option<MarksContext>,
}

/// What an answer to the question marks: a summary, and the lines on request.
#[derive(Serialize)]
struct MarksContext {
    /// "Will mark 4 lines reviewed · 20 lines not relevant".
    summary: String,
    reviewed: Vec<String>,
    /// Each with why it is not relevant.
    not_relevant: Vec<String>,
    reopened: Vec<String>,
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
    fn new(round: &'a RoundSnapshot, notice: Option<&'a Notice>, dev_version: Option<u64>) -> Self {
        let mut context = Self {
            revision: round.revision,
            stage: Stage::NoRound,
            question: None,
            conclusion: None,
            failure: None,
            notice,
            dev_version,
        };
        match &round.stage {
            RoundStage::NoRound => {}
            RoundStage::AgentWorking => context.stage = Stage::Working,
            RoundStage::Question {
                number,
                question,
                citations,
                marks,
            } => {
                context.stage = Stage::Question;
                context.question = Some(QuestionContext::new(*number, question, citations, marks));
            }
            RoundStage::Interrupted { failure } => {
                context.stage = Stage::Interrupted;
                context.failure = failure.as_deref();
            }
            RoundStage::Conclusion(conclusion) => {
                context.stage = Stage::Conclusion;
                context.conclusion = Some(conclusion);
            }
        }
        context
    }
}

impl<'a> QuestionContext<'a> {
    fn new(
        number: usize,
        question: &'a Question,
        citations: &'a [Citation],
        marks: &QuestionMarks,
    ) -> Self {
        Self {
            number,
            id: &question.id,
            version: question.version,
            text: &question.text,
            context: question.context(),
            sections: question
                .assessments
                .iter()
                .flat_map(Assessments::sections)
                .collect(),
            choices: question.choices().collect(),
            citations: citations.iter().map(CitationContext::new).collect(),
            marks: MarksContext::new(marks),
        }
    }
}

impl MarksContext {
    fn new(marks: &QuestionMarks) -> Option<Self> {
        let summary = marks.counts().summary(MarkTense::Pending);
        if summary.is_empty() {
            return None;
        }
        Some(Self {
            summary,
            reviewed: marks.reviewed.iter().map(ToString::to_string).collect(),
            not_relevant: marks.not_relevant.iter().map(ToString::to_string).collect(),
            reopened: marks.reopened.iter().map(ToString::to_string).collect(),
        })
    }
}

/// What the page's script polls while the agent works.
async fn status(Admitted(round): Admitted) -> Response {
    let revision = round.stages.latest().revision;
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
