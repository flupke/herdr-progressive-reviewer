//! The diff viewer's search: the query, its matches across every loaded
//! document, and the background request that computes them.
//!
//! The viewer passes in its documents and the reviewer's input, and applies the
//! returned [`Intent`]s in order: it moves the cursor, starts or cancels
//! background work, and republishes the search status.

use std::sync::Arc;

use text_search::{Position, Request, Results};
use ui_events::SearchStatusChanged;

/// A place in the searched documents, ordered by document, then position.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Location {
    pub document_index: usize,
    pub path: String,
    pub position: Position,
}

/// One document the viewer shows, as search sees it.
pub trait SearchedDocument {
    fn path(&self) -> &str;

    /// The presented text; search asks for it only when a query needs it.
    fn text(&self) -> Arc<text_search::Document>;
}

/// Which way to move from the cursor to the next match.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    Forward,
    Backward,
}

/// One change the reviewer makes while typing the query.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Edit<'a> {
    Type(char),
    Backspace,
    /// Pasted text joins the query on one line.
    Paste(&'a str),
    /// Keep the query and its matches, and stop editing.
    Accept,
    /// Drop the search and go back to where it started.
    Cancel,
}

/// What the viewer must do after a search input, in order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Intent {
    /// Move the cursor; `remember` records the move in location history.
    Jump { to: Location, remember: bool },
    /// Hand this request to the background worker.
    Request(Request),
    /// Stop the background request this search was waiting for.
    CancelRequest,
    /// Republish the search status line, and with `files` the list of files
    /// that contain matches.
    Status { files: bool },
}

/// The diff viewer's search, inactive until the reviewer begins one.
#[derive(Debug, Default)]
pub struct Search {
    active: Option<ActiveSearch>,
}

#[derive(Debug)]
struct ActiveSearch {
    query: String,
    origin: Location,
    editing: bool,
    waiting_for_repository: bool,
    matches: Vec<Location>,
    pending: Option<Request>,
    matched: Option<Request>,
    navigate_on_results: bool,
}

impl Search {
    /// Start typing a new query from `origin`, replacing any previous search.
    pub fn begin_typing<D: SearchedDocument>(
        &mut self,
        origin: Location,
        documents: &[D],
    ) -> Vec<Intent> {
        self.begin(String::new(), true, origin, documents)
    }

    /// Search for `word` (the word under the cursor at `origin`) and move
    /// straight to its next occurrence, replacing any previous search.
    pub fn find_word<D: SearchedDocument>(
        &mut self,
        word: String,
        origin: Location,
        documents: &[D],
    ) -> Vec<Intent> {
        self.begin(word, false, origin, documents)
    }

    fn begin<D: SearchedDocument>(
        &mut self,
        query: String,
        editing: bool,
        origin: Location,
        documents: &[D],
    ) -> Vec<Intent> {
        let mut intents = self.cancel_pending();
        let mut search = ActiveSearch {
            query,
            origin,
            editing,
            waiting_for_repository: false,
            matches: Vec::new(),
            pending: None,
            matched: None,
            navigate_on_results: false,
        };
        intents.extend(search.refresh(documents));
        search.navigate_on_results = true;
        if !editing {
            let origin = search.origin.clone();
            intents.extend(search.step(Direction::Forward, &origin));
        }
        self.active = Some(search);
        intents.push(Intent::Status { files: true });
        intents
    }

    /// Apply one edit to the query being typed; ignored once editing ends.
    pub fn edit<D: SearchedDocument>(&mut self, edit: Edit<'_>, documents: &[D]) -> Vec<Intent> {
        let Some(search) = self.active.as_mut().filter(|search| search.editing) else {
            return Vec::new();
        };
        match edit {
            Edit::Type(character) => search.query.push(character),
            Edit::Backspace => {
                search.query.pop();
            }
            Edit::Paste(text) => search.query.push_str(&text.replace(['\n', '\r'], " ")),
            Edit::Accept => {
                search.editing = false;
                return vec![Intent::Status { files: false }];
            }
            Edit::Cancel => return self.cancel(),
        }
        let mut intents = search.refresh(documents).into_iter().collect::<Vec<_>>();
        intents.extend(search.jump_from_origin());
        intents.push(Intent::Status { files: true });
        intents
    }

    fn cancel(&mut self) -> Vec<Intent> {
        let mut intents = self.cancel_pending();
        intents.extend(self.active.take().map(|search| Intent::Jump {
            to: search.origin,
            remember: false,
        }));
        intents.push(Intent::Status { files: true });
        intents
    }

    /// Move from `current` to the next or previous match, wrapping around.
    pub fn step(&self, direction: Direction, current: Option<&Location>) -> Vec<Intent> {
        let jump = self
            .active
            .as_ref()
            .filter(|search| !search.editing)
            .zip(current)
            .and_then(|(search, current)| search.step(direction, current));
        jump.into_iter()
            .chain([Intent::Status { files: true }])
            .collect()
    }

    /// Drop the search without moving the cursor.
    pub fn clear(&mut self) -> Vec<Intent> {
        let mut intents = self.cancel_pending();
        self.active = None;
        intents.push(Intent::Status { files: true });
        intents
    }

    /// Recompute matches after the documents changed.
    pub fn refresh<D: SearchedDocument>(&mut self, documents: &[D]) -> Vec<Intent> {
        self.active
            .as_mut()
            .and_then(|search| search.refresh(documents))
            .into_iter()
            .collect()
    }

    /// Recompute matches after one more document finished loading, and keep
    /// following the first match while the query or the repository is still
    /// changing.
    pub fn documents_loaded<D: SearchedDocument>(&mut self, documents: &[D]) -> Vec<Intent> {
        let Some(search) = &mut self.active else {
            return Vec::new();
        };
        let mut intents = search.refresh(documents).into_iter().collect::<Vec<_>>();
        if search.editing || search.waiting_for_repository {
            intents.extend(search.jump_from_origin());
        }
        intents
    }

    /// The viewer started loading every document so the search covers them.
    pub fn wait_for_repository(&mut self) {
        if let Some(search) = &mut self.active {
            search.waiting_for_repository = true;
        }
    }

    /// No document is loading any more.
    pub fn repository_loaded(&mut self) {
        if let Some(search) = &mut self.active {
            search.waiting_for_repository = false;
        }
    }

    /// Continue after the viewer was hidden: the shared worker may have
    /// replaced this search's request with another viewer's.
    pub fn resume<D: SearchedDocument>(&mut self, documents: &[D]) -> Vec<Intent> {
        match self
            .active
            .as_ref()
            .and_then(|search| search.pending.clone())
        {
            Some(request) => vec![Intent::Request(request)],
            None => self.refresh(documents),
        }
    }

    /// Accept background results in the viewer the reviewer is looking at.
    ///
    /// Results for another request are ignored. Results computed from
    /// documents that changed since are replaced by a fresh request.
    pub fn complete<D: SearchedDocument>(
        &mut self,
        results: &Results,
        documents: &[D],
    ) -> Vec<Intent> {
        let Some((search, freshness)) = self.awaited(results, documents) else {
            return Vec::new();
        };
        if freshness == Freshness::Stale {
            let navigate = search.navigate_on_results;
            let intents = search.refresh(documents);
            search.navigate_on_results = navigate;
            return intents.into_iter().collect();
        }
        search.accept(results, documents);
        let mut intents = Vec::new();
        if search.navigate_on_results {
            intents.extend(search.jump_from_origin());
        }
        intents.push(Intent::Status { files: true });
        intents
    }

    /// Keep background results in a viewer the reviewer is not looking at,
    /// without moving it.
    pub fn retain<D: SearchedDocument>(&mut self, results: &Results, documents: &[D]) {
        let Some((search, freshness)) = self.awaited(results, documents) else {
            return;
        };
        match freshness {
            Freshness::Current => search.accept(results, documents),
            Freshness::Stale => {
                search.pending = None;
                search.matched = None;
            }
        }
    }

    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    pub fn is_editing(&self) -> bool {
        self.active.as_ref().is_some_and(|search| search.editing)
    }

    pub fn query(&self) -> Option<&str> {
        self.active.as_ref().map(|search| search.query.as_str())
    }

    /// The status line for a cursor at `current`.
    pub fn status(&self, current: Option<&Location>) -> SearchStatusChanged {
        let Some(search) = &self.active else {
            return SearchStatusChanged::default();
        };
        let current_match = current
            .and_then(|current| search.matches.iter().position(|found| found == current))
            .map_or(0, |index| index.saturating_add(1));
        SearchStatusChanged {
            query: Some(search.query.clone()),
            current_match,
            total_matches: search.matches.len(),
        }
    }

    /// Paths with at least one match, in document order.
    pub fn match_paths(&self) -> Vec<String> {
        let Some(search) = &self.active else {
            return Vec::new();
        };
        let mut paths = search
            .matches
            .iter()
            .map(|found| found.path.clone())
            .collect::<Vec<_>>();
        paths.dedup();
        paths
    }

    fn cancel_pending(&self) -> Vec<Intent> {
        self.active
            .as_ref()
            .and_then(|search| search.pending.as_ref())
            .map(|_| Intent::CancelRequest)
            .into_iter()
            .collect()
    }

    /// Whether the search is waiting for these results, and whether they
    /// still describe its documents.
    fn awaited<D: SearchedDocument>(
        &mut self,
        results: &Results,
        documents: &[D],
    ) -> Option<(&mut ActiveSearch, Freshness)> {
        let search = self.active.as_mut()?;
        let request = search
            .pending
            .as_ref()
            .filter(|request| request.id() == results.id)?;
        let freshness = if request.same_documents(&texts(documents)) {
            Freshness::Current
        } else {
            Freshness::Stale
        };
        Some((search, freshness))
    }
}

/// Whether awaited results were computed from the current documents.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Freshness {
    Current,
    Stale,
}

impl ActiveSearch {
    fn refresh<D: SearchedDocument>(&mut self, documents: &[D]) -> Option<Intent> {
        let texts = if self.query.is_empty() {
            Vec::new()
        } else {
            texts(documents)
        };
        if self
            .pending
            .as_ref()
            .or(self.matched.as_ref())
            .is_some_and(|request| request.query == self.query && request.same_documents(&texts))
        {
            return None;
        }
        let cancel = self.pending.take().is_some();
        self.matched = None;
        self.matches.clear();
        if self.query.is_empty() {
            return cancel.then_some(Intent::CancelRequest);
        }
        let request = Request::new(self.query.clone(), texts);
        if request.is_large() {
            self.navigate_on_results = self.editing || self.waiting_for_repository;
            self.pending = Some(request.clone());
            Some(Intent::Request(request))
        } else {
            self.replace_matches(request.search().matches, documents);
            self.matched = Some(request);
            cancel.then_some(Intent::CancelRequest)
        }
    }

    fn accept<D: SearchedDocument>(&mut self, results: &Results, documents: &[D]) {
        self.matched = self.pending.take();
        self.replace_matches(results.matches.clone(), documents);
    }

    fn replace_matches<D: SearchedDocument>(
        &mut self,
        matches: Vec<text_search::Match>,
        documents: &[D],
    ) {
        self.matches = matches
            .into_iter()
            .filter_map(|found| {
                let document = documents.get(found.document_index)?;
                Some(Location {
                    document_index: found.document_index,
                    path: document.path().to_owned(),
                    position: found.position,
                })
            })
            .collect();
    }

    /// Follow the query from where the search started.
    fn jump_from_origin(&self) -> Option<Intent> {
        let to = if self.query.is_empty() {
            Some(self.origin.clone())
        } else {
            self.first_after(&self.origin).cloned()
        };
        to.map(|to| Intent::Jump {
            to,
            remember: false,
        })
    }

    fn step(&self, direction: Direction, current: &Location) -> Option<Intent> {
        let to = match direction {
            Direction::Forward => self.first_after(current),
            Direction::Backward => self.first_before(current),
        }?;
        Some(Intent::Jump {
            to: to.clone(),
            remember: true,
        })
    }

    fn first_after(&self, reference: &Location) -> Option<&Location> {
        self.matches
            .iter()
            .find(|location| *location > reference)
            .or_else(|| self.matches.first())
    }

    fn first_before(&self, reference: &Location) -> Option<&Location> {
        self.matches
            .iter()
            .rev()
            .find(|location| *location < reference)
            .or_else(|| self.matches.last())
    }
}

fn texts<D: SearchedDocument>(documents: &[D]) -> Vec<Arc<text_search::Document>> {
    documents.iter().map(SearchedDocument::text).collect()
}

#[cfg(test)]
#[path = "lib.tests.rs"]
mod tests;
