//! One interview question at a time, with retained history and native evidence windows.
use comment_editor::{CommentEditor, KeymapSetting};
use component_core::{Component, ComponentSubscriptions, EventPublisher};
use review_explore::{AnswerInput, Command, Exploration, Question, RoundFront};
use review_explore_page_opening::PaneStarts;
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
};
use ui_actions::{Action, ExplorePageAction};
use ui_events::{
    EvidenceView, ExploreCaptured, ExploreComparisonAccepted, ExploreEvidence, ExploreFinished,
    ExploreStartBlock, ReviewNavigation, ReviewNavigationChanged,
};
use ui_shortcuts::{MovementShortcut, ShortcutMatcher};

mod adaptive;
mod cancel;
mod choices;
mod composer;
mod conclusion;
mod controls;
mod evidence;
mod flow;
mod input;
mod navigation;
mod network_page;
mod page_actions;
mod page_round;
mod page_start;
mod persistence;
mod render;
mod reset;
mod start;
mod unopened_page;
use flow::ConversationLayout;

#[derive(Clone, Copy, Debug)]
enum Control {
    /// Choose where the reviewer follows a round: on the Explore page or in the pane.
    Front(start::FrontControl),
    /// Ask to close the round and return to the start screen.
    Reset,
    /// Confirm a Reset asked for within the last five seconds.
    ConfirmReset,
    NewImplementation,
    Send,
    History(navigation::History),
    Map,
    Cancel,
    Retry,
    /// Cancel this answer, the latest one.
    CancelAnswer(usize),
    /// Show or hide the review marks this conversation turn changed.
    Marks(usize),
    Reply(usize),
    GeneralReply,
    SelectChoice(usize),
    Evidence(EvidenceView),
    FirstEvidence(EvidenceView),
    Edit,
    EditImplementation,
    Implement,
    CancelImplementation,
}

use review_explore::{ExplorePage as DraftKey, QuestionReading as TurnView};

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
    RestoreScroll,
    Editor(EditorTarget),
    Choice,
    Evidence,
    KeepAnswer { offset: isize },
}

#[derive(Clone, Copy, PartialEq)]
enum ComposeScope {
    Question,
    Conclusion,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum EditorTarget {
    #[default]
    Answer,
    Implementation,
}

#[allow(
    clippy::struct_excessive_bools,
    reason = "Independent UI expansion and focus states"
)]
pub struct ExploreComponent {
    events: EventPublisher,
    durable: persistence::Durability,
    exploration: Option<Exploration>,
    mode: ReviewNavigation,
    selected: usize,
    turns: Vec<TurnView>,
    editor: CommentEditor,
    keymap: KeymapSetting,
    editing: bool,
    evidence_list_focused: bool,
    evidence_keys: ShortcutMatcher<MovementShortcut>,
    drafts: BTreeMap<DraftKey, CommentEditor>,
    editor_target: EditorTarget,
    conclusions: BTreeMap<String, conclusion::ConclusionView>,
    compose_scope: ComposeScope,
    general_context: Option<String>,
    status: String,
    status_turn: Option<usize>,
    progress: Progress,
    reset: reset::ResetConfirmation,
    /// The round being started has a challenger.
    challenger: bool,
    /// The round being started opens its Explore page once its change is captured.
    open_page: bool,
    /// What Start and Start with Challenger do, as the settings say.
    pane_starts: PaneStarts,
    /// Where the reviewer follows the round being started or shown.
    front: RoundFront,
    map: bool,
    /// The review marks each agent turn changed, by Explore request.
    marks: BTreeMap<String, review_explore::TurnMarks>,
    /// Requests whose marks are listed line by line.
    expanded_marks: BTreeSet<String>,
    /// The answer whose cancellation the session is working on.
    cancelling: Option<String>,
    /// Implementation was requested, so answers can no longer be cancelled.
    implementation_requested: bool,
    scroll: Cell<usize>,
    reveal: Cell<Option<Reveal>>,
    heights: BTreeMap<EvidenceView, u16>,
    evidence_width: Option<u16>,
    layout: RefCell<ConversationLayout>,
    drag: Option<input::ResizeDrag>,
    split_drag: bool,
    pointer_view: Option<flow::Window>,
    /// The page on the network, once the page host announced it.
    network_page: Option<network_page::NetworkPage>,
    /// Why the browser could not open the page of the round the pane started last.
    unopened_page: Option<review_explore_page_opening::PageNotOpened>,
    /// Why no round can start, as the session last said: the start buttons are inactive.
    start_block: Option<review_explore::StartBlock>,
}

impl ExploreComponent {
    /// Create the component with editors that share an application-wide keymap.
    pub fn with_keymap(events: EventPublisher, keymap: KeymapSetting) -> Self {
        Self {
            editor: CommentEditor::new("", &keymap),
            keymap,
            events,
            durable: persistence::Durability::default(),
            exploration: None,
            mode: ReviewNavigation::Files,
            selected: 0,
            turns: Vec::new(),
            editing: false,
            evidence_list_focused: false,
            evidence_keys: ShortcutMatcher::new(),
            drafts: BTreeMap::new(),
            editor_target: EditorTarget::Answer,
            conclusions: BTreeMap::new(),
            compose_scope: ComposeScope::Question,
            general_context: None,
            status: "Start a question-first review of the working copy.".into(),
            status_turn: None,
            progress: Progress::Ready,
            reset: reset::ResetConfirmation::default(),
            challenger: false,
            open_page: false,
            pane_starts: PaneStarts::default(),
            front: RoundFront::Pane,
            map: false,
            marks: BTreeMap::new(),
            expanded_marks: BTreeSet::new(),
            cancelling: None,
            implementation_requested: false,
            scroll: Cell::new(0),
            reveal: Cell::new(None),
            heights: BTreeMap::new(),
            evidence_width: None,
            layout: RefCell::default(),
            drag: None,
            split_drag: false,
            pointer_view: None,
            network_page: None,
            unopened_page: None,
            start_block: None,
        }
    }

    fn general_reply(&self) -> bool {
        self.compose_scope == ComposeScope::Conclusion
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
        if !self.progress.can_submit() || self.durable.blocked() {
            return Vec::new();
        }
        let question = self.question().cloned();
        let contributed = input.is_some();
        let Some(exploration) = &mut self.exploration else {
            return Vec::new();
        };
        self.status_turn = question.as_ref().map(|_| self.selected);
        let result = if self.durable.enabled {
            exploration.clone().request(input, question.as_ref())
        } else {
            exploration.request(input, question.as_ref())
        };
        match result {
            Ok(request) => {
                if contributed && !self.durable.enabled {
                    self.editor = CommentEditor::new("", &self.keymap);
                    self.drafts.remove(&self.draft_key());
                }
                if self.durable.enabled {
                    self.durable.posting = Some(request.clone());
                }
                self.status = if self.durable.enabled {
                    "Saving answer and preparing the agent turn…"
                } else {
                    "Waiting for the implementation agent…"
                }
                .into();
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
        let open_page = std::mem::take(&mut self.open_page);
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
                let mut exploration = Exploration::new(comparison.clone());
                exploration.challenger = self.challenger;
                self.open_round(exploration);
                let mut actions = self.request(None);
                if open_page {
                    actions.push(Action::ExplorePage(ExplorePageAction::Open));
                }
                actions
            }
            Err(error) => {
                // With nothing left to review, the inactive starts show in place of Retry, and
                // the line under them says why.
                self.status = if self.start_block.is_some() {
                    self.failed_start("The round could not start", error)
                } else {
                    error.clone()
                };
                self.progress = Progress::Retryable;
                Vec::new()
            }
        }
    }

    /// Show the new round `exploration`, from its first page, before its kickoff.
    fn open_round(&mut self, exploration: Exploration) {
        self.durable.begin_round();
        self.implementation_requested = false;
        self.cancelling = None;
        let comparison = exploration.comparison.clone();
        self.exploration = Some(exploration);
        self.selected = 0;
        self.evidence_list_focused = false;
        self.turns.clear();
        self.drafts.clear();
        self.heights.clear();
        self.compose_scope = ComposeScope::Question;
        self.general_context = None;
        self.conclusions.clear();
        self.editor_target = EditorTarget::Answer;
        self.scroll.set(0);
        self.editor = CommentEditor::new("", &self.keymap);
        self.events.publish(ExploreComparisonAccepted(comparison));
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
                    return Err("This Explore round is being replaced".into());
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
            if self.durable.enabled {
                self.events
                    .publish(ui_events::ReviewPaneFocusRequested(self.durable.focus));
            }
        }
    }

    fn select(&mut self, index: usize) {
        self.save_draft();
        self.open_question(index);
    }

    /// Show question `index` with its draft, leaving the current draft as is.
    fn open_question(&mut self, index: usize) {
        self.compose_scope = ComposeScope::Question;
        self.selected = index;
        self.editing = false;
        self.evidence_list_focused = false;
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
        let EvidenceView { turn, .. } = view;
        if let Some(exploration) = &self.exploration
            && exploration.questions.get(turn).is_some()
        {
            self.events.publish(ExploreEvidence {
                comparison: exploration.comparison.clone(),
                evidence: exploration.evidence(turn),
                view,
                reveal,
            });
        }
    }

    /// Start a round from the start screen as `start` asks: on the page, it opens once the
    /// change is captured, unless the settings keep every start in the pane.
    fn start(&mut self, start: start::RoundStart) -> Vec<Action> {
        let front = match self.pane_starts {
            PaneStarts::OnPage => start.front,
            PaneStarts::InPane => RoundFront::Pane,
        };
        self.capture(start.challenger, front, front == RoundFront::Page)
    }

    /// Ask the session to capture the change for a new round followed at `front`, whose page
    /// opens once the change is captured when `open_page` says so.
    fn capture(&mut self, challenger: bool, front: RoundFront, open_page: bool) -> Vec<Action> {
        if self.exploration.is_some()
            || self.progress.awaiting_capture()
            || self.awaiting_page_start()
            || self.durable.error.is_some()
            || self.start_block.is_some()
        {
            return Vec::new();
        }
        self.challenger = challenger;
        self.front = front;
        self.open_page = open_page;
        self.unopened_page = None;
        self.progress = Progress::Capturing;
        self.status = "Preparing the complete working-copy comparison…".into();
        vec![Action::Explore(Command::Start)]
    }

    fn answer(&mut self, control: Control) -> Vec<Action> {
        if self.cancelling.is_some() {
            return Vec::new();
        }
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
            in_reply_to: self
                .general_reply()
                .then(|| self.general_context.clone())
                .flatten(),
            // The pane shows the recommendation at once: no pick is blind.
            first_pick: None,
        }))
    }

    fn retry(&mut self) -> Vec<Action> {
        if self.progress.awaiting_capture()
            || self.durable.blocked()
            || self.durable.posting.is_some()
        {
            return Vec::new();
        }
        let Some(exploration) = &mut self.exploration else {
            // Retry starts the same round again, without opening its page.
            return self.capture(self.challenger, self.front, false);
        };
        match exploration.retry() {
            Ok(request) => {
                if self.durable.enabled {
                    self.durable.posting = Some(request.clone());
                }
                self.status = "Retrying interview turn…".into();
                self.progress = Progress::Waiting;
                vec![Action::Explore(Command::Retry(Box::new(request)))]
            }
            Err(error) => {
                self.status = error.to_string();
                Vec::new()
            }
        }
    }

    fn start_block_set(&mut self, event: &ExploreStartBlock) {
        self.start_block = event.0;
    }

    /// Show or hide the provisional map or one answer's marks.
    fn toggle(&mut self, control: Control) {
        match control {
            Control::Map => self.map = !self.map,
            Control::Marks(turn) => self.toggle_marks(turn),
            _ => {}
        }
    }

    /// Show or hide the lines the marks of one conversation turn changed.
    fn toggle_marks(&mut self, turn: usize) {
        let Some(id) = self
            .exploration
            .as_ref()
            .and_then(|exploration| exploration.conversation.get(turn))
            .map(|turn| turn.update.request.clone())
        else {
            return;
        };
        if !self.expanded_marks.remove(&id) {
            self.expanded_marks.insert(id);
        }
    }
}

impl Component<Action> for ExploreComponent {
    fn register_subscriptions(subscriptions: &mut ComponentSubscriptions<'_, Self, Action>) {
        subscriptions.subscribe(|component: &mut Self, event: &ui_events::ExploreAutosave| {
            component.autosave(event).into_iter().collect::<Vec<_>>()
        });
        subscriptions.subscribe(Self::history_changed);
        subscriptions.subscribe(Self::restored);
        subscriptions.subscribe(Self::posted);
        subscriptions.subscribe(Self::committed);
        subscriptions.subscribe(Self::answer_cancelled);
        subscriptions.subscribe(Self::storage_failed);
        subscriptions.subscribe(Self::implementation_saved);
        subscriptions.subscribe(Self::captured);
        subscriptions.subscribe(Self::finished);
        subscriptions.subscribe(Self::submitted);
        subscriptions.subscribe(Self::navigation);
        subscriptions.subscribe(Self::implementation_finished);
        subscriptions.subscribe(Self::expiration_tick);
        subscriptions.subscribe(Self::page_shared);
        subscriptions.subscribe(Self::page_not_shared);
        subscriptions.subscribe(Self::page_start);
        subscriptions.subscribe(Self::page_not_opened);
        subscriptions.subscribe(Self::pane_starts_set);
        subscriptions.subscribe(Self::start_block_set);
        subscriptions.subscribe(Self::page_stopped);
        subscriptions.subscribe(Self::page_reset);
        Self::register_input(subscriptions);
    }
}
