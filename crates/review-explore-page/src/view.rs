//! What the page's client draws: the round as typed data, built from the latest stage the
//! round's owner published. The client draws every screen from a [`PageView`]; the agent's
//! Markdown travels rendered to HTML (fields named `..._html`), and cited code as rows of
//! tokens with their colour roles. The TypeScript declarations of these types are generated
//! (`crate::typescript`), so that the client is checked against them.

mod conclusion;
mod question;

use markdown_html::HtmlRenderer;
use review_explore::{Design, QuestionSection, RailStep, TabTitle};
use serde::Serialize;
use ts_rs::TS;

pub(crate) use self::conclusion::ConclusionView;
pub(crate) use self::question::QuestionView;
use crate::blind::FirstPicks;
use crate::round::{LatestAnswer, ReviewName, RoundSnapshot, RoundStage, TurnResponse};
use crate::status::StatusCard;

/// The round as the page shows it.
#[derive(Debug, Serialize, TS)]
pub(crate) struct PageView {
    /// The review the page belongs to, once its owner named it.
    review: Option<ReviewName>,
    /// The start cover, when no round is running.
    start: Option<StartView>,
    /// The status cards above the round's stage: that the round is an earlier one, and the
    /// stage when it is not a question. A refused action's notice travels in its reply.
    cards: Vec<StatusCard>,
    /// The reviewer's latest answer, when the page offers to cancel it.
    cancellable: Option<LatestAnswer>,
    /// The number on the rail of the question the latest answer answered, when known.
    answered: Option<usize>,
    /// The design of the change, as the round's first turn explained it.
    design: Option<DesignView>,
    /// What the agent's turn said back to the reviewer's previous answer, above its question or
    /// conclusion; `None` when it said nothing.
    response: Option<ResponseView>,
    question: Option<QuestionView>,
    conclusion: Option<ConclusionView>,
    /// The round Reset closes, when the page offers it.
    reset: Option<String>,
    /// Whether the round is an earlier one, which the reviewer can only Reset.
    earlier: bool,
    /// The address of Mermaid's script, which draws the diagrams.
    mermaid: String,
    /// The steps of the round rail, in order; empty when no round is running.
    rail: Vec<RailStep>,
    /// What the browser tab's title says before the review's name; `None` when no round is
    /// running, and when the review tool cannot save the round, which then waits for nothing.
    title: Option<TabTitle>,
}

/// The start cover, when no round is running.
#[derive(Debug, Serialize, TS)]
struct StartView {
    /// The identity of the start the cover offers, which a Start carries back.
    start: String,
    /// Whether no status card says the cover's state, so that the cover says that no round is
    /// running.
    idle: bool,
    /// The ID of the status card that says why the reviewer cannot start a round: the start
    /// buttons are inactive, and it describes them.
    block: Option<String>,
}

/// One titled part of the agent's Markdown, rendered: its lead, which shows while the part is
/// folded, then the rest.
#[derive(Debug, Serialize, TS)]
pub(crate) struct SectionView {
    title: String,
    lead_html: String,
    /// `None` when the part is its lead alone.
    details_html: Option<String>,
}

impl SectionView {
    /// `section`, rendered to sit under a heading of `level`.
    pub(crate) fn new(section: &QuestionSection, level: usize) -> Self {
        Self {
            title: section.title.to_owned(),
            lead_html: markdown(&section.lead, level),
            details_html: markdown_if_any(&section.details, level),
        }
    }
}

/// The design of the change, which the design screen shows: it opens the round, and the
/// reviewer can open it again at any later stage.
#[derive(Debug, Serialize, TS)]
struct DesignView {
    /// The change's thesis, as inline HTML for a heading.
    thesis_html: String,
    /// The four parts, in reading order.
    parts: Vec<DesignPartView>,
    /// About how many minutes the design takes to read.
    minutes: usize,
    /// How many files the change touches.
    changed_files: usize,
}

/// One part of the design, under its heading: its thesis, then the rest of its text.
#[derive(Debug, Serialize, TS)]
struct DesignPartView {
    title: String,
    /// The part's thesis, as inline HTML for one line.
    thesis_html: String,
    body_html: String,
}

/// What the agent's turn said back to the reviewer's previous answer.
#[derive(Debug, Serialize, TS)]
struct ResponseView {
    interpretations: Vec<InterpretationView>,
    reply_html: Option<String>,
}

/// The agent's recap of the answer, and the follow-ups it recorded.
#[derive(Debug, Serialize, TS)]
struct InterpretationView {
    recap_html: Option<String>,
    follow_ups: Vec<String>,
}

impl PageView {
    /// The view of `round`, with the reviewer's first picks `picks` kept by this page.
    pub(crate) fn new(round: &RoundSnapshot, picks: &FirstPicks) -> Self {
        // An earlier round offers Reset only.
        let offers_actions = !round.earlier;
        let cards = StatusCard::above_stage(round, offers_actions);
        let start = round.stage.offered_start().map(|start| StartView {
            start: start.to_owned(),
            idle: !StatusCard::say_start(&cards),
            block: StatusCard::start_block(&cards).map(str::to_owned),
        });
        let storage_failed = matches!(round.stage, RoundStage::StorageFailed { .. });
        Self {
            review: round.review.clone(),
            start,
            cancellable: round
                .cancellable
                .clone()
                .filter(|_| offers_actions && !storage_failed),
            answered: round.answered_number(),
            design: round
                .design
                .as_deref()
                .map(|design| DesignView::new(design, round.changed_files)),
            response: round.stage.response().map(ResponseView::new),
            question: QuestionView::of(round, picks),
            conclusion: ConclusionView::of(round, offers_actions),
            reset: round.round.clone().filter(|_| !storage_failed),
            earlier: round.earlier,
            cards,
            mermaid: crate::diagram::script_path(),
            rail: round
                .overview
                .as_ref()
                .map_or_else(Vec::new, |overview| overview.rail.clone()),
            title: round
                .overview
                .as_ref()
                .filter(|_| !storage_failed)
                .map(|overview| overview.title),
        }
    }
}

impl DesignView {
    /// The design as `Design::thesis` and `Design::parts` give it, which stand in for the theses
    /// of a design saved before it had any, of a change of `changed_files` files.
    fn new(design: &Design, changed_files: usize) -> Self {
        let inline = |text: &str| {
            HtmlRenderer::under_heading(3)
                .render_inline(text)
                .into_string()
        };
        Self {
            thesis_html: inline(&design.thesis()),
            parts: design
                .parts()
                .into_iter()
                .map(|part| DesignPartView {
                    title: part.title.to_owned(),
                    thesis_html: inline(&part.thesis),
                    body_html: markdown(&part.body, 3),
                })
                .collect(),
            minutes: design.reading_minutes(),
            changed_files,
        }
    }
}

impl ResponseView {
    fn new(response: &TurnResponse) -> Self {
        Self {
            interpretations: response
                .interpretations
                .iter()
                .map(|interpretation| InterpretationView {
                    recap_html: markdown_if_any(&interpretation.recap, 2),
                    follow_ups: interpretation.follow_ups.clone(),
                })
                .collect(),
            reply_html: response.reply.as_deref().map(|reply| markdown(reply, 2)),
        }
    }
}

/// The agent's Markdown `text` as HTML that sits under a heading of `level`.
pub(crate) fn markdown(text: &str, level: usize) -> String {
    HtmlRenderer::under_heading(level)
        .render(text)
        .into_string()
}

/// `text` as HTML under a heading of `level`, or `None` when it is empty.
pub(crate) fn markdown_if_any(text: &str, level: usize) -> Option<String> {
    (!text.is_empty()).then(|| markdown(text, level))
}
