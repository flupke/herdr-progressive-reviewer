//! Portable reading and editor state, separate from accepted investigation content.
use review_types::TextEditorState;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Question indices address the immutable posted question/version list within a pass.
#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq, Ord, PartialOrd)]
pub enum ExplorePage {
    #[default]
    Opening,
    Question(usize),
    Conclusion(String),
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
pub struct ExploreDraft {
    pub editor: TextEditorState,
    pub correction: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "Independently expanded sections"
)]
pub struct QuestionReading {
    pub reference: usize,
    pub choice: usize,
    pub more: bool,
    pub references: bool,
    pub supporting: bool,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
pub enum EditorFocus {
    #[default]
    Answer,
    Implementation,
    Evidence,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
pub struct EvidencePosition {
    pub turn: usize,
    pub reference: usize,
    pub scroll: usize,
    pub cursor: usize,
    pub column: usize,
}

/// The reviewer's saved editors and reading position for one pass.
#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "Independent saved expansion and editor states"
)]
pub struct ExploreViewState {
    pub page: ExplorePage,
    pub turns: Vec<QuestionReading>,
    pub drafts: Vec<(ExplorePage, ExploreDraft)>,
    pub tasks: BTreeMap<String, TextEditorState>,
    pub replies: BTreeMap<String, bool>,
    pub editing: bool,
    pub focus: EditorFocus,
    pub scroll: usize,
    pub map: bool,
    #[serde(default)]
    pub coverage_overview: bool,
    #[serde(default)]
    pub coverage_file: Option<usize>,
    #[serde(default)]
    pub coverage_next: BTreeMap<usize, usize>,
    #[serde(default)]
    pub jev_debug: bool,
    pub heights: Vec<((usize, usize), u16)>,
    pub code: Vec<EvidencePosition>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
pub struct ViewSave {
    pub review_unit: review_types::ReviewUnit,
    pub instance: String,
    pub sequence: u64,
    pub state: ExploreViewState,
}

impl crate::Exploration {
    pub fn answer_page(&self, answer: &crate::ReviewerAnswer) -> Option<ExplorePage> {
        match &answer.question {
            Some(question) => self
                .questions
                .iter()
                .position(|q| q == question)
                .map(ExplorePage::Question),
            None => Some(ExplorePage::Conclusion(answer.in_reply_to.clone())),
        }
    }
}

impl crate::ExplorePass {
    /// Ignore only the exact pre-post draft consumed by a durable answer.
    pub fn restored_view(&self, view: Option<&ViewSave>) -> ExploreViewState {
        let Some(view) = view else {
            return ExploreViewState {
                page: self.exploration.conclusion_request().map_or_else(
                    || {
                        self.exploration
                            .questions
                            .len()
                            .checked_sub(1)
                            .map_or(ExplorePage::Opening, ExplorePage::Question)
                    },
                    |id| ExplorePage::Conclusion(id.to_owned()),
                ),
                ..Default::default()
            };
        };
        let mut state = view.state.clone();
        for delivery in self.turns.values() {
            if delivery
                .editor_sequence
                .is_some_and(|sequence| view.sequence <= sequence)
                && let Some(answer) = &delivery.request.answer
            {
                let target = self.exploration.answer_page(answer);
                state.drafts.retain(|(page, draft)| {
                    Some(page) != target.as_ref()
                        || draft.editor.text != answer.text
                        || draft.correction != answer.corrects
                });
            }
        }
        state
    }
}
