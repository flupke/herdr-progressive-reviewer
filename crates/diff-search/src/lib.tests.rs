use super::*;

struct TestDocument {
    path: &'static str,
    text: Arc<text_search::Document>,
}

impl TestDocument {
    fn new(path: &'static str, rows: &[&str]) -> Self {
        Self {
            path,
            text: Arc::new(text_search::Document::from_rows(
                rows.iter().map(|row| (*row).to_owned()),
            )),
        }
    }

    /// Big enough that searching it goes to the background worker.
    fn large(path: &'static str) -> Self {
        let filler = "x".repeat(70_000);
        Self::new(path, &["needle deleted", &format!("{filler} needle")])
    }
}

impl SearchedDocument for TestDocument {
    fn path(&self) -> &str {
        self.path
    }

    fn text(&self) -> Arc<text_search::Document> {
        Arc::clone(&self.text)
    }
}

/// The query or its matches changed.
const STATUS: Intent = Intent::Status { files: true };

fn location(document_index: usize, path: &str, row: usize, column: usize) -> Location {
    Location {
        document_index,
        path: path.to_owned(),
        position: Position { row, column },
    }
}

fn jump(to: Location) -> Intent {
    Intent::Jump {
        to,
        remember: false,
    }
}

fn visit(to: Location) -> Intent {
    Intent::Jump { to, remember: true }
}

fn small_documents() -> Vec<TestDocument> {
    vec![
        TestDocument::new("first.rs", &["---"]),
        TestDocument::new("second.rs", &["needle one needle"]),
        TestDocument::new("third.rs", &["needle"]),
    ]
}

fn type_query(search: &mut Search, query: &str, documents: &[TestDocument]) -> Vec<Intent> {
    query
        .chars()
        .flat_map(|character| search.edit(Edit::Type(character), documents))
        .collect()
}

fn requests(intents: &[Intent]) -> Vec<Request> {
    intents
        .iter()
        .filter_map(|intent| match intent {
            Intent::Request(request) => Some(request.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn typing_follows_the_first_match_after_the_origin() {
    let documents = small_documents();
    let origin = location(0, "first.rs", 0, 0);
    let mut search = Search::default();
    assert_eq!(search.begin_typing(origin.clone(), &documents), [STATUS]);

    let intents = search.edit(Edit::Type('n'), &documents);

    assert_eq!(intents, [jump(location(1, "second.rs", 0, 0)), STATUS]);
    assert_eq!(search.query(), Some("n"));
    assert_eq!(
        search.status(Some(&location(1, "second.rs", 0, 0))),
        SearchStatusChanged {
            query: Some("n".to_owned()),
            current_match: 1,
            total_matches: 4,
        }
    );
    assert_eq!(search.match_paths(), ["second.rs", "third.rs"]);
    let erased = search.edit(Edit::Backspace, &documents);
    assert_eq!(erased, [jump(origin), STATUS]);
    assert_eq!(search.match_paths(), Vec::<String>::new());
}

#[test]
fn pasted_text_joins_the_query_on_one_line_only_while_editing() {
    let documents = [TestDocument::new("a.rs", &["one two three"])];
    let mut search = Search::default();
    search.begin_typing(location(0, "a.rs", 0, 0), &documents);

    search.edit(Edit::Paste("two\r\nthree"), &documents);

    assert_eq!(search.query(), Some("two  three"));
    assert_eq!(
        search.edit(Edit::Accept, &documents),
        [Intent::Status { files: false }]
    );
    assert!(!search.is_editing());
    assert!(search.edit(Edit::Paste("more"), &documents).is_empty());
    assert_eq!(search.query(), Some("two  three"));
}

#[test]
fn next_and_previous_wrap_in_document_order_and_are_remembered() {
    let documents = small_documents();
    let mut search = Search::default();
    search.begin_typing(location(0, "first.rs", 0, 0), &documents);
    type_query(&mut search, "needle", &documents);
    let current = location(1, "second.rs", 0, 0);
    assert_eq!(
        search.step(Direction::Forward, Some(&current)),
        [STATUS],
        "stepping waits until the query is accepted"
    );
    search.edit(Edit::Accept, &documents);

    let forward = search.step(Direction::Forward, Some(&current));
    let wrapped = search.step(Direction::Forward, Some(&location(2, "third.rs", 0, 0)));
    let backward = search.step(Direction::Backward, Some(&current));

    assert_eq!(forward, [visit(location(1, "second.rs", 0, 11)), STATUS]);
    assert_eq!(wrapped, [visit(current.clone()), STATUS]);
    assert_eq!(backward, [visit(location(2, "third.rs", 0, 0)), STATUS]);
    assert_eq!(
        search.step(Direction::Forward, None),
        [STATUS],
        "no cursor, no move"
    );
}

#[test]
fn searching_the_word_under_the_cursor_moves_to_its_next_occurrence() {
    let documents = small_documents();
    let mut search = Search::default();

    let intents = search.find_word(
        "needle".to_owned(),
        location(1, "second.rs", 0, 0),
        &documents,
    );

    assert_eq!(intents, [visit(location(1, "second.rs", 0, 11)), STATUS]);
    assert!(!search.is_editing());
}

#[test]
fn cancel_restores_the_origin_and_stops_background_work() {
    let documents = [TestDocument::large("large.rs")];
    let origin = location(0, "large.rs", 1, 3);
    let mut search = Search::default();
    search.begin_typing(origin.clone(), &documents);
    let typed = type_query(&mut search, "needle", &documents);
    assert_eq!(requests(&typed).len(), "needle".len());

    let cancelled = search.edit(Edit::Cancel, &documents);

    assert_eq!(cancelled, [Intent::CancelRequest, jump(origin), STATUS]);
    assert!(!search.is_active());
    assert_eq!(search.status(None), SearchStatusChanged::default());
}

#[test]
fn superseded_results_are_discarded() {
    let documents = [TestDocument::large("large.rs")];
    let mut search = Search::default();
    search.begin_typing(location(0, "large.rs", 0, 0), &documents);
    let old = requests(&search.edit(Edit::Type('n'), &documents)).remove(0);
    let current = requests(&type_query(&mut search, "eedle", &documents))
        .pop()
        .unwrap();

    assert!(search.complete(&old.search(), &documents).is_empty());
    assert_eq!(search.status(None).total_matches, 0);

    let completed = search.complete(&current.search(), &documents);
    assert_eq!(
        completed,
        [jump(location(0, "large.rs", 1, 70_001)), STATUS]
    );
    assert_eq!(search.status(None).total_matches, 2);
    assert!(
        search.complete(&current.search(), &documents).is_empty(),
        "results arrive once"
    );
}

#[test]
fn results_after_cancel_or_clear_are_discarded() {
    let documents = [TestDocument::large("large.rs")];
    let mut search = Search::default();
    search.begin_typing(location(0, "large.rs", 0, 0), &documents);
    let request = requests(&type_query(&mut search, "needle", &documents))
        .pop()
        .unwrap();

    assert_eq!(search.clear(), [Intent::CancelRequest, STATUS]);

    assert!(search.complete(&request.search(), &documents).is_empty());
    assert_eq!(search.clear(), [STATUS]);
}

#[test]
fn results_for_changed_documents_are_requested_again() {
    let mut search = Search::default();
    let before = [TestDocument::large("large.rs")];
    search.begin_typing(location(0, "large.rs", 0, 0), &before);
    let request = requests(&type_query(&mut search, "needle", &before))
        .pop()
        .unwrap();
    let after = [TestDocument::large("large.rs")];

    let retried = requests(&search.complete(&request.search(), &after)).remove(0);

    assert_ne!(retried.id(), request.id());
    assert!(retried.same_documents(&[after[0].text()]));
    assert_eq!(
        search.complete(&retried.search(), &after),
        [jump(location(0, "large.rs", 1, 70_001)), STATUS],
        "the retried results still follow the query"
    );
}

#[test]
fn a_hidden_viewer_keeps_results_without_moving() {
    let documents = [TestDocument::large("large.rs")];
    let mut search = Search::default();
    search.begin_typing(location(0, "large.rs", 0, 0), &documents);
    let request = requests(&type_query(&mut search, "needle", &documents))
        .pop()
        .unwrap();
    assert_eq!(
        search.resume(&documents),
        [Intent::Request(request.clone())]
    );

    search.retain(&request.search(), &documents);

    assert_eq!(search.status(None).total_matches, 2);
    assert!(
        search.resume(&documents).is_empty(),
        "nothing left to resume"
    );
}

#[test]
fn a_hidden_viewer_drops_results_for_changed_documents() {
    let before = [TestDocument::large("large.rs")];
    let mut search = Search::default();
    search.begin_typing(location(0, "large.rs", 0, 0), &before);
    let request = requests(&type_query(&mut search, "needle", &before))
        .pop()
        .unwrap();
    let after = [TestDocument::large("large.rs")];

    search.retain(&request.search(), &after);

    assert_eq!(search.status(None).total_matches, 0);
    assert_eq!(requests(&search.resume(&after)).len(), 1);
}

#[test]
fn beginning_again_cancels_the_previous_background_work() {
    let documents = [TestDocument::large("large.rs")];
    let mut search = Search::default();
    search.begin_typing(location(0, "large.rs", 0, 0), &documents);
    type_query(&mut search, "needle", &documents);

    let intents = search.begin_typing(location(0, "large.rs", 0, 0), &documents);

    assert_eq!(intents, [Intent::CancelRequest, STATUS]);
}

#[test]
fn loading_documents_follows_the_first_match_only_while_it_matters() {
    let mut documents = vec![TestDocument::new("first.rs", &["no match"])];
    let mut search = Search::default();
    search.begin_typing(location(0, "first.rs", 0, 0), &documents);
    type_query(&mut search, "needle", &documents);
    search.edit(Edit::Accept, &documents);
    documents.push(TestDocument::new("second.rs", &["needle"]));

    assert!(
        search.documents_loaded(&documents).is_empty(),
        "an accepted query no longer follows new matches"
    );

    search.wait_for_repository();
    documents.push(TestDocument::new("third.rs", &["needle"]));
    assert_eq!(
        search.documents_loaded(&documents),
        [jump(location(1, "second.rs", 0, 0))]
    );
    search.repository_loaded();
    documents.push(TestDocument::new("fourth.rs", &["needle"]));
    assert!(search.documents_loaded(&documents).is_empty());
    assert_eq!(search.status(None).total_matches, 3);
}
