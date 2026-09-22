//! Review-wide conversation navigation, independent of the file tree.

use component_core::{
    AnyInput, Component, ComponentSubscriptions, EventPublisher, InputMatcher, InputResolution,
    InputScope,
};
use review_threads::{Resolution, ReviewThread, ReviewThreads, ThreadId};
use review_types::ReviewUnit;
use ui_actions::Action;
use ui_events::{
    FilesViewportChanged, NewRepliesRequested, PointerInput, PointerInputKind,
    RepositoryFilesChanged, ReviewNavigation, ReviewNavigationChanged, ReviewPane,
    ReviewPaneFocusRequested, ReviewThreadsLoaded, ThreadSelectionChanged,
};
use ui_shortcuts::Key;

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

/// The Files/Threads mode and a stable list of conversations for the whole review.
pub struct ThreadsComponent {
    events: EventPublisher,
    mode: ReviewNavigation,
    review_unit: Option<ReviewUnit>,
    book: Option<ReviewThreads>,
    associations: review_threads::ThreadPaths,
    files: Vec<ui_events::FileSummary>,
    contexts: std::collections::HashMap<ThreadId, ui_events::ThreadContext>,
    selected: Option<ThreadId>,
    filter: Filter,
    query: String,
    searching: bool,
    scroll: usize,
    viewport_rows: usize,
    focus_request: (u64, ReviewPane),
}

impl ThreadsComponent {
    const CARD_HEIGHT: usize = 6;
    const FOOTER_HEIGHT: usize = 1;

    pub fn new(events: EventPublisher) -> Self {
        Self {
            events,
            mode: ReviewNavigation::Files,
            review_unit: None,
            book: None,
            files: Vec::new(),
            associations: review_threads::ThreadPaths::default(),
            contexts: std::collections::HashMap::new(),
            selected: None,
            filter: Filter::Unresolved,
            query: String::new(),
            searching: false,
            scroll: 0,
            viewport_rows: 1,
            focus_request: (0, ReviewPane::Navigation),
        }
    }

    pub fn mode(&self) -> ReviewNavigation {
        self.mode
    }

    pub fn has_unread_replies(&self) -> bool {
        self.book
            .as_ref()
            .is_some_and(|book| book.counts().unread > 0)
    }

    /// A serial number lets the application consume each focus request once.
    pub fn focus_request(&self) -> (u64, ReviewPane) {
        self.focus_request
    }

    fn visible(&self) -> Vec<&ReviewThread> {
        let query = self.query.to_lowercase();
        self.book
            .iter()
            .flat_map(ReviewThreads::threads)
            .filter(|thread| {
                let matches_filter = match self.filter {
                    Filter::Unresolved => thread.resolution == Resolution::Open,
                    Filter::All => true,
                };
                matches_filter
                    && (query.is_empty()
                        || thread.path().to_lowercase().contains(&query)
                        || thread
                            .messages
                            .iter()
                            .any(|message| message.text.to_lowercase().contains(&query)))
            })
            .collect()
    }

    fn select(&mut self, id: Option<ThreadId>) {
        self.selected = id;
        self.events.publish(ThreadSelectionChanged {
            thread_id: self.selected.clone(),
        });
        self.keep_selected_visible();
    }

    fn keep_selected_visible(&mut self) {
        if let Some(index) = self
            .visible()
            .iter()
            .position(|thread| Some(&thread.id) == self.selected.as_ref())
        {
            if index < self.scroll {
                self.scroll = index;
            }
            if index >= self.scroll + self.page_rows() {
                self.scroll = index + 1 - self.page_rows();
            }
        }
    }

    fn move_selection(&mut self, delta: isize) {
        let visible = self.visible();
        let current = visible
            .iter()
            .position(|thread| Some(&thread.id) == self.selected.as_ref())
            .unwrap_or_default();
        let index = current
            .saturating_add_signed(delta)
            .min(visible.len().saturating_sub(1));
        let id = visible.get(index).map(|thread| thread.id.clone());
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
        self.select(self.visible().first().map(|thread| thread.id.clone()));
    }

    fn key(&mut self, key: Key) -> Vec<Action> {
        if self.searching {
            self.edit_search(key);
            return Vec::new();
        }
        match key {
            Key::Down | Key::Char('j') => self.move_selection(1),
            Key::Up | Key::Char('k') => self.move_selection(-1),
            Key::PageDown | Key::HalfPageDown => {
                self.move_selection(isize::try_from(self.page_rows()).unwrap_or(isize::MAX));
            }
            Key::PageUp | Key::HalfPageUp => {
                self.move_selection(-isize::try_from(self.page_rows()).unwrap_or(isize::MAX));
            }
            Key::First => self.move_selection(isize::MIN),
            Key::Last => self.move_selection(isize::MAX),
            _ => self.action_key(key),
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
            Key::Space => self.query.push(' '),
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

    fn action_key(&mut self, key: Key) {
        match key {
            Key::Char('/') => {
                self.searching = true;
                self.keep_selected_visible();
            }
            Key::Char('1') => self.choose_filter(Filter::Unresolved),
            Key::Char('2') => self.choose_filter(Filter::All),
            Key::Enter | Key::Right | Key::Char('l') => {
                self.select(self.selected.clone());
                self.events
                    .publish(ReviewPaneFocusRequested(ReviewPane::Detail));
            }
            _ => {}
        }
    }

    fn repository_changed(&mut self, event: &RepositoryFilesChanged) {
        if self.review_unit.as_ref() != Some(&event.review_checkpoint.review_unit) {
            self.book = None;
            self.contexts.clear();
            self.selected = None;
            self.query.clear();
            self.scroll = 0;
            self.review_unit = Some(event.review_checkpoint.review_unit.clone());
        }
        self.files.clone_from(&event.files);
        self.associations = ui_events::FileSummary::thread_paths(&self.files);
    }

    fn file_for_thread(&self, thread: &ReviewThread) -> Option<&ui_events::FileSummary> {
        self.files
            .iter()
            .find(|file| file.path() == self.associations.resolve(thread.path()))
    }

    fn loaded(&mut self, event: &ReviewThreadsLoaded) {
        if self.review_unit.as_ref() != Some(&event.review_unit) {
            return;
        }
        if let Ok(book) = &event.result {
            let visible = self.visible();
            let was_empty = visible.is_empty();
            let position = visible
                .iter()
                .position(|thread| Some(&thread.id) == self.selected.as_ref())
                .unwrap_or_default();
            let resolved = self.selected.as_ref().is_some_and(|id| {
                self.book
                    .as_ref()
                    .and_then(|previous| previous.thread(id))
                    .is_some_and(|thread| thread.resolution == Resolution::Open)
                    && book
                        .thread(id)
                        .is_some_and(|thread| thread.resolution == Resolution::Resolved)
            });
            self.book = Some(book.clone());
            if resolved {
                self.select(self.next_unresolved(position));
            } else if self.selected.as_ref().map_or(was_empty, |id| {
                !self.visible().iter().any(|thread| thread.id == *id)
            }) {
                self.select(self.visible().first().map(|thread| thread.id.clone()));
            }
        }
    }

    fn next_unresolved(&self, position: usize) -> Option<ThreadId> {
        let visible = self.visible();
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
        self.focus_request = (self.focus_request.0.wrapping_add(1), event.0);
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
        let visible = self.visible();
        let id = visible
            .iter()
            .find(|thread| thread.has_unread_replies())
            .or_else(|| visible.first())
            .map(|thread| thread.id.clone());
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

    fn contexts_changed(&mut self, event: &ui_events::ThreadContextsChanged) {
        if self.review_unit.as_ref() == Some(&event.review_unit) {
            self.contexts.clone_from(&event.contexts);
        }
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
                let id = self.visible().get(index).map(|thread| thread.id.clone());
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

struct ThreadKeys;

impl InputMatcher<ThreadsComponent, Key> for ThreadKeys {
    type Output = Key;
    fn resolve(&mut self, component: &ThreadsComponent, key: &Key) -> InputResolution<Key> {
        if component.searching
            || matches!(
                key,
                Key::Down
                    | Key::Up
                    | Key::PageDown
                    | Key::PageUp
                    | Key::HalfPageDown
                    | Key::HalfPageUp
                    | Key::First
                    | Key::Last
                    | Key::Enter
                    | Key::Right
                    | Key::Char('j' | 'k' | 'l' | '/' | '1' | '2')
            )
        {
            InputResolution::Matched(*key)
        } else {
            InputResolution::NoMatch
        }
    }
}

impl Component<Action> for ThreadsComponent {
    fn register_subscriptions(subscriptions: &mut ComponentSubscriptions<'_, Self, Action>) {
        subscriptions.subscribe(Self::contexts_changed);
        subscriptions.subscribe(Self::review_saved);
        subscriptions.subscribe(Self::repository_changed);
        subscriptions.subscribe(Self::loaded);
        subscriptions.subscribe(Self::navigation_changed);
        subscriptions.subscribe(Self::selected);
        subscriptions.subscribe(Self::focus_requested);
        subscriptions.subscribe(Self::new_replies);
        subscriptions.subscribe(Self::viewport_changed);
        subscriptions.subscribe_input(InputScope::Focused, ThreadKeys, Self::key);
        subscriptions.subscribe_input(
            InputScope::Focused,
            AnyInput,
            |component, input: ui_events::TextPasted| component.paste_search(&input),
        );
        subscriptions.subscribe_input(InputScope::Hovered, AnyInput, Self::navigate_pointer);
    }
}
