//! A question the reviewer answered earlier in the round, as the page shows it from the round
//! rail: the question as it was asked, what the reviewer answered and what the agent recorded,
//! read only. It carries no identity an answer or another action could post with, so nothing
//! on its screen can change the round.

use std::sync::Arc;

use review_explore::{Assessments, Door, EarlierQuestion, KeptAnswer, MarkPhrase, MarkTense};
use review_explore_citations::Citation;
use serde::Serialize;
use ts_rs::TS;

use super::question::CitationView;
use super::{ResponseView, SectionView, markdown, markdown_if_any};

/// A question the reviewer answered earlier in the round, as it was answered: read only.
#[derive(Debug, Serialize, TS)]
pub(crate) struct EarlierQuestionView {
    /// The question's step on the round rail, from 1.
    number: usize,
    /// The question's latest version, as the reviewer answered it.
    text_html: String,
    /// How hard the decision is to reverse, as the agent assessed it; `None` when it did not.
    door: Option<Door>,
    /// The Context section; `None` when the question has none.
    context_html: Option<String>,
    /// The Door and Blast radius sections, folded to their leads.
    sections: Vec<SectionView>,
    /// The answer the reviewer kept; `None` for a question the round left unanswered.
    answer: Option<KeptAnswer>,
    /// What the agent recorded of the answer, and its reply.
    recorded: ResponseView,
    /// What the answers to the question marked, one summary for each turn that marked lines:
    /// "Marked", then "12 lines reviewed", "3 lines not relevant".
    marks: Vec<MarkPhrase>,
    /// The question's citations, most decisive first.
    citations: Vec<CitationView>,
}

impl EarlierQuestionView {
    /// The view of `record`, whose citations, with their lines, are `citations`.
    fn new(record: &EarlierQuestion, citations: &[Citation]) -> Self {
        let question = &record.question;
        Self {
            number: record.number,
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
            answer: record.answer.clone(),
            recorded: ResponseView::of_record(&record.recorded),
            marks: record
                .marks
                .iter()
                .map(|marks| marks.counts().phrase(MarkTense::Applied))
                .filter(|phrase| !phrase.parts.is_empty())
                .collect(),
            citations: citations.iter().map(CitationView::new).collect(),
        }
    }

    /// The earlier questions of `records`, each with its list of `citations`, in the same order.
    pub(crate) fn all(records: &[EarlierQuestion], citations: &[Arc<[Citation]>]) -> Vec<Self> {
        records
            .iter()
            .enumerate()
            .map(|(index, record)| {
                Self::new(record, citations.get(index).map_or(&[][..], |list| list))
            })
            .collect()
    }
}
