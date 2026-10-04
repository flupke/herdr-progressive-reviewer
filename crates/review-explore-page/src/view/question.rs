//! The agent's question, as the page shows it: the text, the explanation, the choices with the
//! agent's recommendation (hidden until the reviewer's first pick on a blind question), the
//! lines an answer marks, and the citations.

use review_explore::{Alternative, Assessments, Door, MarkPhrase, MarkTense, Question};
use review_explore_citations::{Citation, CodeColors, HighlightedRow, Token};
use review_explore_tally::Gain;
use review_repository::diff::DiffRow;
use serde::Serialize;
use ts_rs::TS;

use super::{SectionView, markdown, markdown_if_any};
use crate::blind::{BlindQuestion, FirstPicks};
use crate::round::{AnsweredQuestion, QuestionMarks, RoundSnapshot, RoundStage};

#[derive(Debug, Serialize, TS)]
pub(crate) struct QuestionView {
    /// The identity of the round that asks the question, which the answer carries back: a
    /// question of the same ID in a later round is another question.
    round: Option<String>,
    /// The question's step on the round rail, from 1: a clarified question keeps its number.
    number: usize,
    id: String,
    version: u32,
    text_html: String,
    /// How hard the decision is to reverse, as the agent assessed it; `None` when it did not.
    door: Option<Door>,
    /// The Context section; `None` when the question has none.
    context_html: Option<String>,
    /// The Door and Blast radius sections, folded away until the reviewer opens them, each
    /// showing its lead.
    sections: Vec<SectionView>,
    /// When the page shows the agent's recommendation.
    recommendation: Recommendation,
    /// The agent's alternatives, then None of the above; once the recommendation shows after
    /// the first pick, the choice picked first is the checked one.
    choices: Vec<ChoiceView>,
    /// The question's citations, most decisive first.
    citations: Vec<CitationView>,
    /// The lines an answer marks, `None` when it marks none.
    marks: Option<MarksView>,
    /// The reviewed share of the change before and after the answer; `None` when the tool
    /// cannot tell, as when the code changed since the round started.
    gain: Option<GainView>,
    /// Whether the page offers to answer: not in an earlier round.
    answerable: bool,
}

/// When the page shows the agent's recommendation for a question.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
enum Recommendation {
    /// At once, with the choices in the agent's order.
    Shown,
    /// Not yet, on a blind question: the choices in a mixed order, and a form that sends the
    /// reviewer's first pick.
    HiddenUntilPick,
    /// After the first pick of a blind question: the choices in the same mixed order, the pick
    /// selected, and the answer's form.
    ShownAfterPick,
}

/// One choice of a question.
#[derive(Debug, Serialize, TS)]
pub(super) struct ChoiceView {
    id: String,
    text: String,
    /// Why the agent recommends the choice, when it does and the page shows it.
    recommendation: Option<String>,
    /// Whether the choice is selected: the reviewer's first pick.
    checked: bool,
}

/// What an answer to the question marks: a summary, and the lines on request.
#[derive(Debug, Serialize, TS)]
struct MarksView {
    /// "Answering marks", then "15 lines reviewed": the lines marked not relevant count as
    /// reviewed, as in the meter; the lists below keep them apart.
    summary: MarkPhrase,
    reviewed: Vec<String>,
    /// Each with why it is not relevant.
    not_relevant: Vec<String>,
    reopened: Vec<String>,
}

/// The reviewed share of the change before and after the answer, in whole percent, as the
/// reviewer's file list rounds it: "39% → 50%".
#[derive(Debug, Serialize, TS)]
struct GainView {
    before: u64,
    after: u64,
}

/// One citation: its location, its note, and the cited lines of the change, colored on the
/// server.
#[derive(Debug, Serialize, TS)]
pub(crate) struct CitationView {
    /// The path, side and lines: `src/main.rs new 7-9`.
    location: String,
    /// The path alone: `src/main.rs`.
    path: String,
    /// The side and lines alone: `new 7–9`, or `whole file`.
    span: String,
    notes: String,
    rows: Vec<RowView>,
    /// Why the citation shows no rows.
    limitation: Option<String>,
}

/// One row of the cited lines.
#[derive(Debug, Serialize, TS)]
struct RowView {
    kind: RowKind,
    old_line: Option<u32>,
    new_line: Option<u32>,
    tokens: Vec<TokenView>,
}

#[derive(Clone, Copy, Debug, Serialize, TS)]
#[serde(rename_all = "snake_case")]
enum RowKind {
    Context,
    Added,
    Removed,
}

#[derive(Debug, Serialize, TS)]
struct TokenView {
    text: String,
    /// The palette role of the token's color, or `None` for the page's text color.
    role: Option<String>,
}

impl QuestionView {
    /// The question `round` asks, with the reviewer's first pick of it from `picks`.
    pub(crate) fn of(round: &RoundSnapshot, picks: &FirstPicks) -> Option<Self> {
        let RoundStage::Question {
            question,
            citations,
            marks,
            ..
        } = &round.stage
        else {
            return None;
        };
        let blind = round.stage.blind();
        let picked = blind.as_ref().and_then(|blind| {
            let round = round.round.as_deref()?;
            picks
                .of(round, &question.id, question.version)
                .filter(|choice| blind.offers(choice))
        });
        Some(
            Self::new(
                round.round.clone(),
                round.question_number().unwrap_or_default(),
                question,
                blind.as_ref(),
                citations,
                marks,
                picked.as_deref(),
            )
            .answerable(!round.earlier)
            .gain(round.tally.as_ref().and_then(|tally| tally.gain.as_ref())),
        )
    }

    /// The question `answered` of the round `round`, number `number` on the rail, as the reviewer
    /// answered it: the page shows it read only, beside the answer.
    pub(crate) fn answered(
        round: Option<String>,
        number: usize,
        answered: &AnsweredQuestion,
    ) -> Self {
        let mut view = Self::new(
            round,
            number,
            &answered.question,
            None,
            &answered.citations,
            &QuestionMarks::default(),
            None,
        )
        .answerable(false);
        if answered.picked_blind {
            view.recommendation = Recommendation::ShownAfterPick;
        }
        view
    }

    /// `blind` is the question when it hides the agent's recommendation until the first pick;
    /// `picked` the choice the reviewer picked first.
    fn new(
        round: Option<String>,
        number: usize,
        question: &Question,
        blind: Option<&BlindQuestion<'_>>,
        citations: &[Citation],
        marks: &QuestionMarks,
        picked: Option<&str>,
    ) -> Self {
        let (recommendation, choices) = match (blind, picked) {
            (None, _) => (Recommendation::Shown, question.choices().collect()),
            (Some(blind), None) => (Recommendation::HiddenUntilPick, blind.choices()),
            (Some(blind), Some(_)) => (Recommendation::ShownAfterPick, blind.choices()),
        };
        let choices: Vec<_> = choices
            .into_iter()
            .map(|choice| ChoiceView::new(choice, recommendation, picked))
            .collect();
        Self {
            round,
            number,
            id: question.id.clone(),
            version: question.version,
            text_html: markdown(&question.text, 2),
            door: question
                .assessments
                .as_ref()
                .map(|assessments| assessments.door),
            context_html: markdown_if_any(&question.context(), 2),
            sections: question
                .assessments
                .iter()
                .flat_map(Assessments::sections)
                .map(|section| SectionView::new(&section, 2))
                .collect(),
            recommendation,
            choices,
            citations: citations.iter().map(CitationView::new).collect(),
            marks: MarksView::new(marks),
            gain: None,
            answerable: true,
        }
    }

    fn answerable(mut self, answerable: bool) -> Self {
        self.answerable = answerable;
        self
    }

    /// The question with the choice `kept` (its text), which the reviewer sent, selected.
    pub(super) fn keeping(mut self, kept: Option<&str>) -> Self {
        for choice in &mut self.choices {
            choice.keep(kept);
        }
        self
    }

    /// The share the answer adds, when the answer marks lines.
    fn gain(mut self, gain: Option<&Gain>) -> Self {
        self.gain = gain.filter(|_| self.marks.is_some()).map(|gain| GainView {
            before: gain.before.percent,
            after: gain.after.percent,
        });
        self
    }
}

impl ChoiceView {
    /// The choices of `question` the reviewer answered, in the agent's order and with its
    /// recommendation, the choice `kept` (its text) selected.
    pub(super) fn kept(question: &Question, kept: Option<&str>) -> Vec<Self> {
        question
            .choices()
            .map(|choice| {
                let mut view = Self::new(choice, Recommendation::Shown, None);
                view.keep(kept);
                view
            })
            .collect()
    }

    /// Selects the choice when it is the one `kept`, by its text, as the round keeps an answer.
    fn keep(&mut self, kept: Option<&str>) {
        self.checked = kept == Some(self.text.as_str());
    }

    fn new(choice: &Alternative, recommendation: Recommendation, first_pick: Option<&str>) -> Self {
        Self {
            id: choice.id.clone(),
            text: choice.text.clone(),
            recommendation: choice
                .recommendation
                .clone()
                .filter(|_| recommendation != Recommendation::HiddenUntilPick),
            checked: first_pick == Some(choice.id.as_str()),
        }
    }
}

impl MarksView {
    fn new(marks: &QuestionMarks) -> Option<Self> {
        // What answering covers as a whole, as the meter counts it; the list of lines keeps the
        // split between reviewed and not relevant.
        let summary = marks.counts().covered_phrase(MarkTense::Answering);
        if summary.parts.is_empty() {
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

impl CitationView {
    pub(crate) fn new(citation: &Citation) -> Self {
        let (rows, limitation) = match &citation.lines {
            Ok(rows) => (
                RowView::dedented(rows.iter().filter_map(RowView::new).collect()),
                None,
            ),
            Err(limitation) => (Vec::new(), Some(limitation.to_string())),
        };
        let location = &citation.evidence.location;
        Self {
            location: location.to_string(),
            path: location.path.display().to_string(),
            span: location.lines.as_ref().map_or_else(
                || "whole file".to_owned(),
                |lines| {
                    let span = Self::span(lines.first_line, lines.last_line);
                    format!("{} {span}", location.side)
                },
            ),
            notes: citation.evidence.notes.clone(),
            rows,
            limitation,
        }
    }
}

impl RowView {
    fn new(row: &HighlightedRow) -> Option<Self> {
        let (kind, old_line, new_line) = match row.diff {
            DiffRow::Context {
                old_line, new_line, ..
            } => (RowKind::Context, Some(old_line), Some(new_line)),
            DiffRow::Delete { old_line, .. } => (RowKind::Removed, Some(old_line), None),
            DiffRow::Add { new_line, .. } => (RowKind::Added, None, Some(new_line)),
            _ => return None,
        };
        Some(Self {
            kind,
            old_line,
            new_line,
            tokens: row.tokens.iter().map(TokenView::new).collect(),
        })
    }
}

impl CitationView {
    /// "7", or "7–9" for the lines `first` to `last`: with an en dash, as the handoff's code frames write it.
    fn span(first: u32, last: u32) -> String {
        if first == last {
            first.to_string()
        } else {
            format!("{first}–{last}")
        }
    }
}

impl RowView {
    /// `rows` without the indentation they all share (design review, finding 15): a phone shows
    /// the code, not the spaces before it. Blank rows do not count.
    fn dedented(mut rows: Vec<Self>) -> Vec<Self> {
        let shared = rows.iter().filter_map(Self::indentation).min().unwrap_or(0);
        if shared > 0 {
            for row in &mut rows {
                row.strip(shared);
            }
        }
        rows
    }

    /// How many spaces or tabs open the row, or `None` for a blank row.
    fn indentation(&self) -> Option<usize> {
        let mut count = 0;
        for character in self.tokens.iter().flat_map(|token| token.text.chars()) {
            match character {
                ' ' | '\t' => count += 1,
                '\n' | '\r' => return None,
                _ => return Some(count),
            }
        }
        None
    }

    /// Takes up to `count` leading spaces or tabs off the row.
    fn strip(&mut self, mut count: usize) {
        for token in &mut self.tokens {
            if count == 0 {
                break;
            }
            let blank = token
                .text
                .chars()
                .take(count)
                .take_while(|character| matches!(character, ' ' | '\t'))
                .count();
            token.text.drain(..blank);
            count -= blank;
            if !token.text.is_empty() {
                break;
            }
        }
        self.tokens.retain(|token| !token.text.is_empty());
    }
}

impl TokenView {
    fn new(token: &Token) -> Self {
        Self {
            text: token.text.clone(),
            role: CodeColors::role(token).map(str::to_owned),
        }
    }
}

#[cfg(test)]
#[path = "question.tests.rs"]
mod tests;
