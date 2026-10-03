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
    Alternative, AnswerInput, Assessments, Conclusion, Design, MarkTense, Question, QuestionSection,
};
use review_explore_citations::Citation;
use serde::{Deserialize, Serialize};

use crate::access::{Hosts, TokenCookie};
use crate::blind::{BlindQuestion, FirstPick};
use crate::citation::CitationContext;
use crate::command::{PageAnswer, PageCommand, PageImplement};
use crate::diagram::{self, Diagrams};
use crate::files::PageFiles;
use crate::form::TextArea;
use crate::notice::{Notice, Post, Problem};
use crate::round::{
    ImplementationState, PageImplementation, PageRound, QuestionMarks, RoundSnapshot, RoundStage,
    Rounds,
};

/// Scripts, styles and form posts only from the page itself, and no inline script. Inline
/// styles are allowed for Mermaid, which writes them into each diagram it draws: without them
/// its boxes and labels are misplaced. The browser reports what the policy blocks to
/// `/csp-report`.
const CONTENT_SECURITY_POLICY: &str = "default-src 'none'; script-src 'self'; \
     style-src 'self' 'unsafe-inline'; connect-src 'self'; img-src 'self'; form-action 'self'; \
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
        let (script_path, script) = diagram::script_route::<R>();
        Router::new()
            .route("/", get(index::<R>))
            .route(&script_path, script)
            .route("/diagram-errors", diagram::report_route::<R>())
            .route("/status", get(status))
            .route("/pick", post(pick))
            .route("/answer", post(answer))
            .route("/start", post(start))
            .route("/implement", post(implement))
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
        to_page(Some(cookie))
    }

    /// Shows the round, with the notice of a post that did not go through, once.
    fn show(&self, headers: &HeaderMap) -> Response {
        let round = match self.admit(headers) {
            Ok(round) => round.stages.latest(),
            Err(refused) => return refused.into_response(),
        };
        let notice = Notice::read(headers);
        let first_pick = match &round.stage {
            RoundStage::Question { question, .. } => {
                FirstPick::read(headers, round.round.as_deref(), question)
            }
            _ => None,
        };
        let context = PageContext::new(
            &round,
            notice.as_ref(),
            first_pick.as_ref(),
            self.files.dev_version(),
        );
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
pub(crate) struct Refused;

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
pub(crate) struct Admitted(pub(crate) PageRound);

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
    comment: TextArea,
}

/// Hands the reviewer's answer to the round's owner, with the reviewer's first pick of a blind
/// question, unless the page showed a question that no longer waits for an answer, then shows
/// the page again with what became of it.
async fn answer(
    Admitted(round): Admitted,
    headers: HeaderMap,
    Form(form): Form<AnswerForm>,
) -> Response {
    let shown = round.stages.latest();
    let sent = if let Some(question) = shown.stage.asks(&form.question, form.version) {
        let first_pick = FirstPick::read(&headers, shown.round.as_deref(), question);
        let answer = PageAnswer {
            question: form.question,
            version: form.version,
            input: AnswerInput {
                option: form.choice,
                text: form.comment.into_string(),
                in_reply_to: None,
                first_pick: first_pick.map(|pick| pick.choice),
            },
        };
        round.commands.send(PageCommand::Answer(answer)).await
    } else {
        Err(Problem::Stale)
    };
    to_page(Some(match sent {
        Ok(()) => FirstPick::clear(),
        Err(problem) => Notice::new(Post::Answer, problem).cookie(),
    }))
}

/// The reviewer's start of a round, as the start form posts it.
#[derive(Deserialize)]
struct StartForm {
    /// Set by Start with Challenger only.
    #[serde(default)]
    challenger: bool,
}

/// Hands the start of a round to the round's owner, unless a round started since the page
/// showed none, then shows the page again with what became of it.
async fn start(Admitted(round): Admitted, Form(form): Form<StartForm>) -> Response {
    let sent = if round.stages.stage().can_start() {
        let challenger = form.challenger;
        round.commands.send(PageCommand::Start { challenger }).await
    } else {
        Err(Problem::Stale)
    };
    to_page(
        sent.err()
            .map(|problem| Notice::new(Post::Start, problem).cookie()),
    )
}

/// The reviewer's first pick of a blind question, as its form posts it.
#[derive(Deserialize)]
struct PickForm {
    question: String,
    version: u32,
    /// The picked choice's ID; absent when the reviewer picked none.
    choice: Option<String>,
}

/// Keeps the reviewer's first pick of a blind question, then shows the page again, now with
/// the agent's recommendation. A pick kept already stays the first one.
async fn pick(
    Admitted(round): Admitted,
    headers: HeaderMap,
    Form(form): Form<PickForm>,
) -> Response {
    let shown = round.stages.latest();
    let asked = shown.stage.asks(&form.question, form.version);
    to_page(match (asked, shown.round.as_deref()) {
        (Some(question), Some(round)) => form
            .choice
            .and_then(|choice| FirstPick::to_keep(&headers, round, question, choice))
            .map(|pick| pick.cookie()),
        _ => Some(Notice::new(Post::Pick, Problem::Stale).cookie()),
    })
}

/// The reviewer's Implement, as the conclusion's form posts it.
#[derive(Deserialize)]
struct ImplementForm {
    conclusion: String,
    /// The delivery of the conclusion's request that the page showed as not sent; absent when
    /// it showed none.
    replaces: Option<String>,
    #[serde(default)]
    text: TextArea,
}

/// Hands the reviewer's Implement to the round's owner, unless the page showed a conclusion
/// that no longer offers it, then shows the page again with what became of it.
async fn implement(Admitted(round): Admitted, Form(form): Form<ImplementForm>) -> Response {
    let offered = round
        .stages
        .stage()
        .offers_implement(&form.conclusion, form.replaces.as_deref());
    let sent = if offered {
        let implement = PageImplement {
            conclusion: form.conclusion,
            replaces: form.replaces,
            text: form.text.into_string(),
        };
        round.commands.send(PageCommand::Implement(implement)).await
    } else {
        Err(Problem::Stale)
    };
    to_page(
        sent.err()
            .map(|problem| Notice::new(Post::Implement, problem).cookie()),
    )
}

/// Redirects to the page, setting `cookie` when given (post, redirect, get).
fn to_page(cookie: Option<HeaderValue>) -> Response {
    let mut response = Redirect::to("/").into_response();
    if let Some(cookie) = cookie {
        response.headers_mut().insert(header::SET_COOKIE, cookie);
    }
    response
}

/// What the template `page.html` receives. The templates turn the agent's Markdown into HTML
/// with the filter `markdown` (see [`PageFiles`]).
#[derive(Serialize)]
struct PageContext<'a> {
    revision: u64,
    stage: Stage,
    /// Whether the page polls its status: while the agent works, or an implementation request
    /// is being sent.
    polls: bool,
    question: Option<QuestionContext<'a>>,
    conclusion: Option<ConclusionContext<'a>>,
    /// Why the turn the agent no longer works on failed, when its prompt failed.
    failure: Option<&'a str>,
    /// Why the reviewer's latest start of a round failed, while no round runs.
    start_failure: Option<&'a str>,
    /// Why the reviewer's latest post did not go through.
    notice: Option<&'a Notice>,
    /// The design of the change, as the round's first turn explained it.
    design: Option<DesignContext>,
    /// Where the page finds Mermaid, and the fence of a diagram block.
    diagrams: Diagrams,
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
    /// When the page shows the agent's recommendation.
    recommendation: Recommendation,
    /// The agent's alternatives, then None of the above.
    choices: Vec<ChoiceContext<'a>>,
    /// The text of the choice the reviewer picked first, once the recommendation shows.
    first_pick: Option<&'a str>,
    /// The question's citations, most decisive first.
    citations: Vec<CitationContext<'a>>,
    /// The lines an answer marks, `None` when it marks none.
    marks: Option<MarksContext>,
}

/// When the page shows the agent's recommendation for a question, as the template tests it.
#[derive(Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Recommendation {
    /// At once, with the choices in the agent's order.
    Shown,
    /// Not yet, on a blind question: the choices in a mixed order, and a form that posts the
    /// reviewer's first pick.
    HiddenUntilPick,
    /// After the first pick of a blind question: the choices in the same mixed order, the pick
    /// selected, and the answer's form.
    ShownAfterPick,
}

/// One choice of a question.
#[derive(Serialize)]
struct ChoiceContext<'a> {
    id: &'a str,
    text: &'a str,
    /// Why the agent recommends the choice, when it does and the page shows it.
    recommendation: Option<&'a str>,
    /// Whether the choice is selected: the reviewer's first pick.
    checked: bool,
}

#[derive(Serialize)]
struct ConclusionContext<'a> {
    /// The request of the agent's turn that posted the conclusion.
    request: &'a str,
    /// The summary and the future work, in Markdown.
    summary: &'a str,
    future_work: &'a str,
    /// The list to be implemented that the form starts from, as raw text: the agent's, or the
    /// reviewer's own list of a request that was not sent.
    draft: &'a str,
    /// Whether the page offers Implement.
    offers_implement: bool,
    /// The latest implementation request of the conclusion.
    implementation: Option<&'a PageImplementation>,
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

/// The design of the change: open above the round's first question, folded away in every
/// later stage of the round.
#[derive(Serialize)]
struct DesignContext {
    open: bool,
    sections: [QuestionSection; 4],
}

impl DesignContext {
    fn new(design: &Design, stage: &RoundStage) -> Self {
        Self {
            open: matches!(stage, RoundStage::Question { number: 1, .. }),
            sections: design.sections(),
        }
    }
}

/// The stage's name, as the template tests it: `no_round`, `starting` and `working` (the page
/// polls its status only then), `start_failed`, `question`, `interrupted` or `conclusion`.
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum Stage {
    NoRound,
    Starting,
    StartFailed,
    Working,
    Question,
    Interrupted,
    Conclusion,
}

impl<'a> PageContext<'a> {
    fn new(
        round: &'a RoundSnapshot,
        notice: Option<&'a Notice>,
        first_pick: Option<&FirstPick>,
        dev_version: Option<u64>,
    ) -> Self {
        let mut context = Self {
            revision: round.revision,
            stage: Stage::NoRound,
            polls: false,
            question: None,
            conclusion: None,
            failure: None,
            start_failure: None,
            notice,
            design: round
                .design
                .as_deref()
                .map(|design| DesignContext::new(design, &round.stage)),
            diagrams: Diagrams::new(),
            dev_version,
        };
        match &round.stage {
            RoundStage::NoRound => {}
            RoundStage::Starting => {
                context.stage = Stage::Starting;
                context.polls = true;
            }
            RoundStage::StartFailed { failure } => {
                context.stage = Stage::StartFailed;
                context.start_failure = Some(failure);
            }
            RoundStage::AgentWorking => {
                context.stage = Stage::Working;
                context.polls = true;
            }
            RoundStage::Question {
                number,
                question,
                citations,
                marks,
            } => {
                context.stage = Stage::Question;
                context.question = Some(QuestionContext::new(
                    *number, question, citations, marks, first_pick,
                ));
            }
            RoundStage::Interrupted { failure } => {
                context.stage = Stage::Interrupted;
                context.failure = failure.as_deref();
            }
            RoundStage::Conclusion {
                request,
                conclusion,
                implementation,
            } => {
                context.stage = Stage::Conclusion;
                context.polls = implementation.as_ref().is_some_and(|implementation| {
                    implementation.state == ImplementationState::Sending
                });
                context.conclusion = Some(ConclusionContext::new(
                    &round.stage,
                    request,
                    conclusion,
                    implementation.as_ref(),
                ));
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
        first_pick: Option<&FirstPick>,
    ) -> Self {
        let blind = BlindQuestion::of(question);
        let first_pick = first_pick
            .filter(|pick| {
                blind
                    .as_ref()
                    .is_some_and(|blind| blind.offers(&pick.choice))
            })
            .map(|pick| pick.choice.as_str());
        let (recommendation, choices) = match (&blind, first_pick) {
            (None, _) => (Recommendation::Shown, question.choices().collect()),
            (Some(blind), None) => (Recommendation::HiddenUntilPick, blind.choices()),
            (Some(blind), Some(_)) => (Recommendation::ShownAfterPick, blind.choices()),
        };
        let choices: Vec<_> = choices
            .into_iter()
            .map(|choice| ChoiceContext::new(choice, recommendation, first_pick))
            .collect();
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
            recommendation,
            first_pick: choices
                .iter()
                .find(|choice| choice.checked)
                .map(|choice| choice.text),
            choices,
            citations: citations.iter().map(CitationContext::new).collect(),
            marks: MarksContext::new(marks),
        }
    }
}

impl<'a> ChoiceContext<'a> {
    fn new(
        choice: &'a Alternative,
        recommendation: Recommendation,
        first_pick: Option<&str>,
    ) -> Self {
        Self {
            id: &choice.id,
            text: &choice.text,
            recommendation: choice
                .recommendation
                .as_deref()
                .filter(|_| recommendation != Recommendation::HiddenUntilPick),
            checked: first_pick == Some(choice.id.as_str()),
        }
    }
}

impl<'a> ConclusionContext<'a> {
    fn new(
        stage: &RoundStage,
        request: &'a str,
        conclusion: &'a Conclusion,
        implementation: Option<&'a PageImplementation>,
    ) -> Self {
        let replaces = implementation.map(|implementation| implementation.delivery.as_str());
        let offers_implement = stage.offers_implement(request, replaces);
        Self {
            request,
            summary: &conclusion.summary,
            future_work: &conclusion.future_work,
            draft: match implementation {
                Some(implementation) if offers_implement => &implementation.text,
                _ => &conclusion.to_be_implemented,
            },
            offers_implement,
            implementation,
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
