//! One interview question at a time, with retained history and native evidence windows.
use comment_editor::{CommentEditor, EditorKeymap};
use component_core::{Component, ComponentSubscriptions, EventPublisher};
use review_explore::{AnswerInput, Command, Exploration, Question};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
};
use ui_actions::Action;
use ui_events::{
    EvidenceView, ExploreCaptured, ExploreComparisonAccepted, ExploreEvidence, ExploreFinished,
    ReviewNavigation, ReviewNavigationChanged,
};

mod adaptive;
mod choices;
mod composer;
mod conclusion;
mod controls;
mod evidence;
mod flow;
mod input;
mod navigation;
mod render;
pub use flow::ConversationLayout;

#[derive(Clone, Copy, Debug)]
enum Control {
    Start,
    Send,
    Defer,
    History(navigation::History),
    Map,
    Cancel,
    Retry,
    Correct(usize),
    Reply(usize),
    GeneralReply,
    SelectChoice(usize),
    Details(usize),
    More(usize),
    References(usize),
    Supporting(usize),
    Evidence(EvidenceView),
    Primary(EvidenceView),
    Fit(EvidenceView),
    Edit,
    EditImplementation,
    Implement,
    CancelImplementation,
}

#[derive(Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "Sections expand independently"
)]
struct TurnView {
    reference: usize,
    choice: usize,
    details: bool,
    more: bool,
    references: bool,
    supporting: bool,
}

impl TurnView {
    fn toggle(&mut self, control: Control) {
        match control {
            Control::Details(_) => self.details = !self.details,
            Control::More(_) => self.more = !self.more,
            Control::References(_) => self.references = !self.references,
            Control::Supporting(_) => self.supporting = !self.supporting,
            _ => {}
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Progress {
    Ready,
    Capturing,
    DiscardingCapture,
    Waiting,
    Retryable,
}

impl Progress {
    fn awaiting_capture(self) -> bool {
        matches!(self, Self::Capturing | Self::DiscardingCapture)
    }

    fn can_submit(self) -> bool {
        matches!(self, Self::Ready | Self::Retryable)
    }
}

#[derive(Clone, Copy)]
enum Reveal {
    Start,
    Editor(EditorTarget),
    Choice,
    KeepAnswer { offset: isize },
}

struct Draft {
    editor: CommentEditor,
    correction: Option<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum ComposeScope {
    Question,
    Opening,
    Conclusion,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum EditorTarget {
    #[default]
    Answer,
    Implementation,
}

#[derive(Eq, PartialEq, Ord, PartialOrd)]
enum DraftKey {
    Question(usize),
    Conclusion(String),
    Opening,
}

pub struct ExploreComponent {
    events: EventPublisher,
    exploration: Option<Exploration>,
    mode: ReviewNavigation,
    selected: usize,
    turns: Vec<TurnView>,
    editor: CommentEditor,
    editing: bool,
    drafts: BTreeMap<DraftKey, Draft>,
    editor_target: EditorTarget,
    conclusions: BTreeMap<String, conclusion::ConclusionView>,
    compose_scope: ComposeScope,
    general_context: Option<String>,
    correction: Option<String>,
    status: String,
    status_turn: Option<usize>,
    progress: Progress,
    reset_warning: bool,
    map: bool,
    scroll: Cell<usize>,
    reveal: Cell<Option<Reveal>>,
    heights: BTreeMap<EvidenceView, u16>,
    layout: RefCell<ConversationLayout>,
    drag: Option<input::ResizeDrag>,
    pointer_view: Option<flow::Window>,
}

impl ExploreComponent {
    pub fn new(events: EventPublisher) -> Self {
        Self {
            events,
            exploration: None,
            mode: ReviewNavigation::Files,
            selected: 0,
            turns: Vec::new(),
            editor: CommentEditor::new("", EditorKeymap::Regular),
            editing: false,
            drafts: BTreeMap::new(),
            editor_target: EditorTarget::Answer,
            conclusions: BTreeMap::new(),
            compose_scope: ComposeScope::Question,
            general_context: None,
            correction: None,
            status: "Start a question-first review of the working copy.".into(),
            status_turn: None,
            progress: Progress::Ready,
            reset_warning: false,
            map: false,
            scroll: Cell::new(0),
            reveal: Cell::new(None),
            heights: BTreeMap::new(),
            layout: RefCell::default(),
            drag: None,
            pointer_view: None,
        }
    }

    fn general_reply(&self) -> bool {
        self.compose_scope != ComposeScope::Question
    }

    fn question(&self) -> Option<&Question> {
        if self.general_reply() {
            return None;
        }
        self.exploration.as_ref()?.questions.get(self.selected)
    }

    fn can_compose(&self) -> bool {
        self.question().is_some()
            || (self.compose_scope == ComposeScope::Conclusion && self.general_context.is_some())
    }

    fn request(&mut self, input: Option<AnswerInput>) -> Vec<Action> {
        if !self.progress.can_submit() {
            return Vec::new();
        }
        let question = self.question().cloned();
        let contributed = input.is_some();
        let Some(exploration) = &mut self.exploration else {
            return Vec::new();
        };
        self.status_turn = question.as_ref().map(|_| self.selected);
        match exploration.request(input, question.as_ref()) {
            Ok(request) => {
                if contributed {
                    self.editor = CommentEditor::new("", self.editor.keymap());
                    self.drafts.remove(&self.draft_key());
                    self.correction = None;
                }
                self.status = "Waiting for the implementation agent…".into();
                self.progress = Progress::Waiting;
                self.editing = false;
                vec![Action::Explore(Command::Turn(Box::new(request)))]
            }
            Err(error) => {
                self.status = error.to_string();
                Vec::new()
            }
        }
    }

    fn captured(&mut self, event: &ExploreCaptured) -> Vec<Action> {
        if self.progress == Progress::DiscardingCapture {
            self.progress = Progress::Retryable;
            self.status = "Cancelled. Your previous questions and text remain available.".into();
            return Vec::new();
        }
        if self.progress != Progress::Capturing {
            return Vec::new();
        }
        self.progress = Progress::Ready;
        match &event.result {
            Ok(comparison) => {
                self.exploration = Some(Exploration::new(comparison.clone()));
                self.selected = 0;
                self.turns.clear();
                self.drafts.clear();
                self.heights.clear();
                self.correction = None;
                self.compose_scope = ComposeScope::Question;
                self.general_context = None;
                self.conclusions.clear();
                self.editor_target = EditorTarget::Answer;
                self.scroll.set(0);
                self.editor = CommentEditor::new("", self.editor.keymap());
                self.events
                    .publish(ExploreComparisonAccepted(comparison.clone()));
                self.request(None)
            }
            Err(error) => {
                self.status.clone_from(error);
                self.progress = Progress::Retryable;
                Vec::new()
            }
        }
    }

    fn finished(&mut self, event: &ExploreFinished) {
        if self.progress.awaiting_capture() {
            return;
        }
        let anchor = self
            .editing
            .then(|| self.layout.borrow().answer_anchor())
            .flatten();
        let Some(exploration) = &mut self.exploration else {
            return;
        };
        if exploration.instance != event.instance {
            return;
        }
        let count = exploration.questions.len();
        let result = event.result.clone().and_then(|update| {
            if update.instance != event.instance || update.request != event.request {
                return Err("Response identity does not match the outstanding request".into());
            }
            exploration.apply(update).map_err(|error| error.to_string())
        });
        match result {
            Ok(true) => {
                self.update_applied(count, anchor);
            }
            Ok(false) => {}
            Err(error) => {
                if exploration.failed(&event.request, &error) {
                    self.status = error;
                    self.progress = Progress::Retryable;
                }
            }
        }
    }

    fn update_applied(&mut self, previous_count: usize, anchor: Option<Reveal>) {
        self.progress = Progress::Ready;
        self.status.clear();
        let count = self
            .exploration
            .as_ref()
            .expect("active exploration")
            .questions
            .len();
        self.turns.resize_with(count, TurnView::default);
        if self
            .exploration
            .as_ref()
            .is_some_and(|exploration| exploration.conclusion.is_some())
        {
            self.accept_conclusion();
        } else if count > previous_count {
            self.select(count - 1);
            if self.mode == ReviewNavigation::Explore {
                self.events.publish(ui_events::ReviewPaneFocusRequested(
                    ui_events::ReviewPane::Navigation,
                ));
            }
        } else if previous_count == 0 {
            self.publish_evidence(self.view_id(), false);
        }
        if self.reveal.get().is_none() {
            self.reveal.set(anchor);
        }
    }

    fn submitted(&mut self, event: &ui_events::ExploreSubmission) {
        let anchor = self
            .editing
            .then(|| self.layout.borrow().answer_anchor())
            .flatten();
        let result = self
            .exploration
            .as_mut()
            .ok_or_else(|| "Start Explore first".to_owned())
            .and_then(|exploration| {
                if self.progress.awaiting_capture() {
                    return Err("This Explore pass is being replaced".into());
                }
                let count = exploration.questions.len();
                exploration
                    .submit(event.update.clone())
                    .map(|applied| (applied, count))
                    .map_err(|error| error.to_string())
            });
        let result = result.map(|(applied, count)| {
            if applied {
                self.update_applied(count, anchor);
            }
            applied
        });
        let _ = event.response.send(result);
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn navigation(&mut self, event: &ReviewNavigationChanged) {
        self.mode = event.0;
        if self.mode == ReviewNavigation::Explore {
            self.publish_evidence(self.view_id(), false);
        }
    }

    fn select(&mut self, index: usize) {
        self.save_draft();
        self.compose_scope = ComposeScope::Question;
        self.selected = index;
        self.editing = false;
        self.editor_target = EditorTarget::Answer;
        self.drag = None;
        self.pointer_view = None;
        self.restore_draft();
        self.reveal.set(Some(Reveal::Start));
        self.publish_evidence(self.view_id(), false);
    }

    fn view_id(&self) -> EvidenceView {
        EvidenceView {
            turn: self.selected,
            reference: self
                .turns
                .get(self.selected)
                .map_or(0, |turn| turn.reference),
        }
    }

    fn publish_evidence(&self, view: EvidenceView, reveal: bool) {
        if let Some(exploration) = &self.exploration
            && exploration.questions.get(view.turn).is_some()
        {
            self.events.publish(ExploreEvidence {
                comparison: exploration.comparison.clone(),
                evidence: exploration.evidence(view.turn),
                primary: exploration.questions[view.turn].evidence.len(),
                view,
                reveal,
            });
        }
    }

    fn start(&mut self) -> Vec<Action> {
        if self.progress.awaiting_capture() {
            return Vec::new();
        }
        if self.exploration.is_some() && !self.reset_warning {
            self.reset_warning = true;
            self.status_turn = Some(self.selected);
            self.status = "New pass discards interview progress and unposted answers. Press n or New pass again to continue; another action cancels.".into();
            return Vec::new();
        }
        self.reset_warning = false;
        self.progress = Progress::Capturing;
        if let Some(exploration) = &mut self.exploration {
            exploration.cancel();
        }
        self.status = "Preparing the complete working-copy comparison…".into();
        vec![Action::Explore(Command::Start)]
    }

    fn answer(&mut self, control: Control) -> Vec<Action> {
        let option = if matches!(control, Control::Send) {
            self.selected_choice().and_then(|index| {
                self.question()?
                    .choices()
                    .nth(index)
                    .map(|option| option.id.clone())
            })
        } else {
            None
        };
        self.request(Some(AnswerInput {
            option,
            text: self.editor.text(),
            deferred: matches!(control, Control::Defer),
            corrects: self.correction.clone(),
            in_reply_to: self
                .general_reply()
                .then(|| self.general_context.clone())
                .flatten(),
        }))
    }

    fn retry(&mut self) -> Vec<Action> {
        if self.progress.awaiting_capture() {
            return Vec::new();
        }
        let Some(exploration) = &mut self.exploration else {
            return self.start();
        };
        match exploration.retry() {
            Ok(request) => {
                self.status = "Retrying interview turn…".into();
                self.progress = Progress::Waiting;
                vec![Action::Explore(Command::Turn(Box::new(request)))]
            }
            Err(error) => {
                self.status = error.to_string();
                Vec::new()
            }
        }
    }

    fn correct(&mut self, turn: usize) {
        if turn != self.selected || self.general_reply() {
            self.select(turn);
        }
        self.correction = self
            .exploration
            .as_ref()
            .and_then(|exploration| {
                exploration
                    .answers
                    .iter()
                    .rev()
                    .find(|answer| answer.question.as_ref() == self.question())
            })
            .map(|answer| answer.id.clone());
        self.editing = self.correction.is_some();
        self.status_turn = Some(turn);
        self.status = "Correction appends to the original answer. Write it and Send.".into();
        self.reveal.set(Some(Reveal::Editor(EditorTarget::Answer)));
    }
}

impl Component<Action> for ExploreComponent {
    fn register_subscriptions(subscriptions: &mut ComponentSubscriptions<'_, Self, Action>) {
        subscriptions.subscribe(Self::captured);
        subscriptions.subscribe(Self::finished);
        subscriptions.subscribe(Self::submitted);
        subscriptions.subscribe(Self::navigation);
        subscriptions.subscribe(Self::implementation_finished);
        Self::register_input(subscriptions);
    }
}
