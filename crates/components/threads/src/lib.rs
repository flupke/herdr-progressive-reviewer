//! Review-wide conversation navigation, independent of the file tree.

use component_core::{
    AnyInput, Component, ComponentSubscriptions, EventPublisher, InputMatcher, InputResolution,
    InputScope,
};
use review_thread_projection::{SharedThreadProjection, ThreadProjection};
use review_threads::{Resolution, ReviewThread, ReviewThreads, ThreadId};
use review_types::ReviewUnit;
use ui_actions::Action;
use ui_events::{
    FilesViewportChanged, NewRepliesRequested, PointerInput, PointerInputKind,
    RepositoryFilesChanged, ReviewNavigation, ReviewNavigationChanged, ReviewPane,
    ReviewPaneFocusRequested, ReviewThreadsLoaded, ThreadSelectionChanged,
};
use ui_shortcuts::{Key, SearchShortcut, ShortcutMatcher, ThreadsCommand, ThreadsShortcut};

mod render;

#[derive(Clone, Copy, Default, Eq, PartialEq)]
enum Filter {
    #[default]
    Unresolved,
    All,
}

impl Filter {
    const ALL: [Self; 2] = [Self::Unresolved, Self::All];

    fn label(self, selected: Self) -> String {
        let label = match self {
            Self::Unresolved => "Unresolved",
            Self::All => "All",
        };
        if self == selected {
            format!("[{label}]")
        } else {
            label.to_owned()
        }
    }
}

/// A stable list of conversations for the whole review.
pub struct ThreadsComponent {
    events: EventPublisher,
    /// The navigation mode as last announced; the application owns it.
    mode: ReviewNavigation,
    review_unit: Option<ReviewUnit>,
    projection: SharedThreadProjection,
    /// The review state of each reviewed file.
    files: Vec<ui_events::FileSummary>,
    selected: Option<ThreadId>,
    filter: Filter,
    query: String,
    searching: bool,
    scroll: usize,
    viewport_rows: usize,
}

impl ThreadsComponent {
    const CARD_HEIGHT: usize = 6;
    const FOOTER_HEIGHT: usize = 1;

    pub fn new(events: EventPublisher, projection: SharedThreadProjection) -> Self {
        Self {
            events,
            mode: ReviewNavigation::Files,
            review_unit: None,
            projection,
            files: Vec::new(),
            selected: None,
            filter: Filter::Unresolved,
            query: String::new(),
            searching: false,
            scroll: 0,
            viewport_rows: 1,
        }
    }

    pub fn has_unread_replies(&self) -> bool {
        self.projection
            .read()
            .threads()
            .is_some_and(|book| book.counts().unread > 0)
    }

    /// The threads the filter and search show, in list order.
    fn visible<'a>(&self, book: Option<&'a ReviewThreads>) -> Vec<&'a ReviewThread> {
        let query = self.query.to_lowercase();
        book.into_iter()
            .flat_map(ReviewThreads::threads)
            .filter(|thread| {
                let matches_filter = match self.filter {
                    Filter::Unresolved => thread.resolution == Resolution::Open,
                    Filter::All => true,
                };
                matches_filter
                    && (query.is_empty()
                        || Self::location(thread).to_lowercase().contains(&query)
                        || thread
                            .messages
                            .iter()
                            .any(|message| message.text.to_lowercase().contains(&query)))
            })
            .collect()
    }

    /// What a thread is about, as its card names it: its file, or its Explore round.
    fn location(thread: &ReviewThread) -> &str {
        thread.path().unwrap_or("Round conversation")
    }

    fn select(&mut self, id: Option<ThreadId>) {
        self.selected = id;
        self.events.publish(ThreadSelectionChanged {
            thread_id: self.selected.clone(),
        });
        self.keep_selected_visible();
    }

    fn keep_selected_visible(&mut self) {
        if let Some(index) = self.visible_position(self.selected.as_ref()) {
            if index < self.scroll {
                self.scroll = index;
            }
            if index >= self.scroll + self.page_rows() {
                self.scroll = index + 1 - self.page_rows();
            }
        }
    }

    /// The position of `id` among the visible threads.
    fn visible_position(&self, id: Option<&ThreadId>) -> Option<usize> {
        let projection = self.projection.read();
        self.visible(projection.threads())
            .iter()
            .position(|thread| Some(&thread.id) == id)
    }

    /// The visible thread at `index`.
    fn visible_id(&self, index: usize) -> Option<ThreadId> {
        let projection = self.projection.read();
        self.visible(projection.threads())
            .get(index)
            .map(|thread| thread.id.clone())
    }

    fn move_selection(&mut self, delta: isize) {
        let id = {
            let projection = self.projection.read();
            let visible = self.visible(projection.threads());
            let index = visible
                .iter()
                .position(|thread| Some(&thread.id) == self.selected.as_ref())
                .unwrap_or_default()
                .saturating_add_signed(delta)
                .min(visible.len().saturating_sub(1));
            visible.get(index).map(|thread| thread.id.clone())
        };
        self.select(id);
    }

    fn choose_filter(&mut self, filter: Filter) {
        self.filter = filter;
        self.searching = false;
        self.reset_selection();
    }

    fn reset_selection(&mut self) {
        self.selected = None;
        self.scroll = 0;
        self.select(self.visible_id(0));
    }

    fn handle_input(&mut self, input: ThreadsInput) -> Vec<Action> {
        match input {
            ThreadsInput::SearchText(key) => self.edit_search(key),
            ThreadsInput::Command(ThreadsCommand::Movement(command)) => {
                let page = isize::try_from(self.page_rows()).unwrap_or(isize::MAX);
                self.move_selection(command.row_delta(page));
            }
            ThreadsInput::Command(ThreadsCommand::Search(SearchShortcut::Begin)) => {
                self.searching = true;
                self.keep_selected_visible();
            }
            ThreadsInput::Command(ThreadsCommand::Threads(command)) => {
                self.run_threads_shortcut(command);
            }
        }
        Vec::new()
    }

    fn edit_search(&mut self, key: Key) {
        match key {
            Key::Escape | Key::Enter => self.searching = false,
            Key::Backspace => {
                self.query.pop();
            }
            Key::Char(character) => self.query.push(character),
            _ => return,
        }
        self.reset_selection();
    }

    fn paste_search(&mut self, event: &ui_events::TextPasted) {
        if !self.searching {
            return;
        }
        self.query.push_str(&event.0.replace(['\n', '\r'], " "));
        self.reset_selection();
    }

    fn run_threads_shortcut(&mut self, command: ThreadsShortcut) {
        match command {
            ThreadsShortcut::ShowUnresolved => self.choose_filter(Filter::Unresolved),
            ThreadsShortcut::ShowAll => self.choose_filter(Filter::All),
            ThreadsShortcut::OpenConversation => {
                self.select(self.selected.clone());
                self.events
                    .publish(ReviewPaneFocusRequested(ReviewPane::Detail));
            }
        }
    }

    fn repository_changed(&mut self, event: &RepositoryFilesChanged) {
        if self.review_unit.as_ref() != Some(&event.review_checkpoint.review_unit) {
            self.selected = None;
            self.query.clear();
            self.scroll = 0;
            self.review_unit = Some(event.review_checkpoint.review_unit.clone());
        }
        self.files.clone_from(&event.files);
    }

    fn file_for_thread(
        &self,
        projection: &ThreadProjection,
        thread: &ReviewThread,
    ) -> Option<&ui_events::FileSummary> {
        let path = projection.current_path(thread.path()?);
        self.files.iter().find(|file| file.path() == path)
    }

    /// Keep a selection the load left visible; otherwise move to the next
    /// open thread when the selected one was just resolved, or to the first.
    fn loaded(&mut self, event: &ReviewThreadsLoaded) {
        if self.review_unit.as_ref() != Some(&event.review_unit) || event.result.is_err() {
            return;
        }
        let projection = self.projection.read();
        let previous = self.visible(projection.previous_threads());
        let was_empty = previous.is_empty();
        let position = previous
            .iter()
            .position(|thread| Some(&thread.id) == self.selected.as_ref())
            .unwrap_or_default();
        let resolution = |book: Option<&ReviewThreads>, id: &ThreadId| {
            book.and_then(|book| book.thread(id))
                .map(|thread| thread.resolution)
        };
        let resolved = self.selected.as_ref().is_some_and(|id| {
            resolution(projection.previous_threads(), id) == Some(Resolution::Open)
                && resolution(projection.threads(), id) == Some(Resolution::Resolved)
        });
        drop(projection);
        if resolved {
            self.select(self.next_unresolved(position));
        } else if self
            .selected
            .as_ref()
            .map_or(was_empty, |id| self.visible_position(Some(id)).is_none())
        {
            self.select(self.visible_id(0));
        }
    }

    fn next_unresolved(&self, position: usize) -> Option<ThreadId> {
        let projection = self.projection.read();
        let visible = self.visible(projection.threads());
        visible
            .iter()
            .find(|thread| thread.resolution == Resolution::Open && thread.has_unread_replies())
            .or_else(|| {
                visible
                    .iter()
                    .skip(position)
                    .chain(visible.iter().take(position))
                    .find(|thread| thread.resolution == Resolution::Open)
            })
            .map(|thread| thread.id.clone())
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn navigation_changed(&mut self, event: &ReviewNavigationChanged) {
        self.mode = event.0;
    }

    fn selected(&mut self, event: &ThreadSelectionChanged) {
        self.selected.clone_from(&event.thread_id);
        self.keep_selected_visible();
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn focus_requested(&mut self, event: &ReviewPaneFocusRequested) {
        if event.0 == ReviewPane::Detail && self.mode == ReviewNavigation::Threads {
            self.select(self.selected.clone());
        }
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn new_replies(&mut self, _: &NewRepliesRequested) {
        self.events
            .publish(ReviewNavigationChanged(ReviewNavigation::Threads));
        self.filter = Filter::All;
        self.query.clear();
        self.searching = false;
        self.scroll = 0;
        let id = {
            let projection = self.projection.read();
            let visible = self.visible(projection.threads());
            visible
                .iter()
                .find(|thread| thread.has_unread_replies())
                .or_else(|| visible.first())
                .map(|thread| thread.id.clone())
        };
        self.select(id);
        self.events
            .publish(ReviewPaneFocusRequested(ReviewPane::Navigation));
    }

    #[allow(clippy::trivially_copy_pass_by_ref)]
    fn viewport_changed(&mut self, event: &FilesViewportChanged) {
        self.viewport_rows = event.rows;
        self.keep_selected_visible();
    }

    fn page_rows(&self) -> usize {
        (self
            .viewport_rows
            .saturating_sub(Self::FOOTER_HEIGHT + self.search_rows())
            / Self::CARD_HEIGHT)
            .max(1)
    }

    fn search_rows(&self) -> usize {
        usize::from(self.searching || !self.query.is_empty())
    }

    fn review_saved(&mut self, event: &ui_events::ReviewStateSaved) {
        if self.review_unit.as_ref() != Some(&event.review_unit) {
            return;
        }
        if let Ok(state) = event.result
            && let Some(file) = self.files.iter_mut().find(|file| file.path() == event.path)
        {
            file.review_state = state;
        }
    }

    fn navigate_pointer(&mut self, input: PointerInput) {
        if let PointerInputKind::Scroll(delta) = input.kind {
            self.move_selection(delta);
        } else if matches!(
            input.kind,
            PointerInputKind::Click | PointerInputKind::DoubleClick
        ) && let Some(position) = input.position
        {
            let row = usize::from(position.component_row);
            if row >= self.viewport_rows {
                return;
            }
            if row == self.viewport_rows - 1 {
                self.click_filter(usize::from(position.component_column));
            } else if row < self.search_rows() {
                self.searching = true;
            } else if row - self.search_rows() < self.page_rows() * Self::CARD_HEIGHT {
                let index = self.scroll + (row - self.search_rows()) / Self::CARD_HEIGHT;
                let id = self.visible_id(index);
                if id.is_some() {
                    self.select(id);
                    self.events
                        .publish(ReviewPaneFocusRequested(ReviewPane::Detail));
                }
            }
        }
    }

    fn click_filter(&mut self, column: usize) {
        let mut start = 0;
        for filter in Filter::ALL {
            let end = start + filter.label(self.filter).len();
            if (start..end).contains(&column) {
                self.choose_filter(filter);
                return;
            }
            start = end + 3;
        }
    }
}

/// A focused key: search text while the query is being edited, otherwise a
/// command from the shortcut table.
#[derive(Clone, Copy)]
enum ThreadsInput {
    SearchText(Key),
    Command(ThreadsCommand),
}

#[derive(Default)]
struct ThreadKeys {
    commands: ShortcutMatcher<ThreadsCommand>,
}

impl InputMatcher<ThreadsComponent, Key> for ThreadKeys {
    type Output = ThreadsInput;

    fn resolve(
        &mut self,
        component: &ThreadsComponent,
        key: &Key,
    ) -> InputResolution<ThreadsInput> {
        if component.searching {
            return InputResolution::Matched(ThreadsInput::SearchText(*key));
        }
        self.commands.resolve_key(*key).map(ThreadsInput::Command)
    }
}

impl Component<Action> for ThreadsComponent {
    fn register_subscriptions(subscriptions: &mut ComponentSubscriptions<'_, Self, Action>) {
        subscriptions.subscribe(Self::review_saved);
        subscriptions.subscribe(Self::repository_changed);
        subscriptions.subscribe(Self::loaded);
        subscriptions.subscribe(Self::navigation_changed);
        subscriptions.subscribe(Self::selected);
        subscriptions.subscribe(Self::focus_requested);
        subscriptions.subscribe(Self::new_replies);
        subscriptions.subscribe(Self::viewport_changed);
        subscriptions.subscribe_input(
            InputScope::Focused,
            ThreadKeys::default(),
            Self::handle_input,
        );
        subscriptions.subscribe_input(
            InputScope::Focused,
            AnyInput,
            |component, input: ui_events::TextPasted| component.paste_search(&input),
        );
        subscriptions.subscribe_input(InputScope::Hovered, AnyInput, Self::navigate_pointer);
    }
}
