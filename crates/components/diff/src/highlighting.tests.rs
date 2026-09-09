use super::*;

struct HighlightFixture {
    syntax: SyntaxHighlighter,
    content: Arc<DiffContentLoaded>,
}

impl HighlightFixture {
    fn new(name: &str) -> Self {
        Self {
            syntax: SyntaxHighlighter::new(EmbeddedThemeName::CatppuccinMocha, Color::White),
            content: Arc::new(DiffContentLoaded {
                review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
                path: "a.rs".to_owned(),
                rows: vec![
                    DiffRow::Hunk {
                        old_start: 2,
                        old_count: 1,
                        new_start: 2,
                        new_count: 1,
                    },
                    DiffRow::Delete {
                        old_line: 2,
                        text: "-fn old() {}".to_owned(),
                    },
                    DiffRow::Add {
                        new_line: 2,
                        text: format!("+fn {name}() {{}}"),
                    },
                ],
                old_content: Some(b"// before\nfn old() {}\n// after\n".to_vec()),
                new_content: Some(format!("// before\nfn {name}() {{}}\n// after\n").into_bytes()),
            }),
        }
    }

    fn load(&self, document: &mut LoadedDocument) {
        document.replace_diff(DiffPresentation::new(self.syntax.plain(
            self.content.rows.clone(),
            self.content.old_content.as_deref(),
            self.content.new_content.as_deref(),
        )));
        document
            .document
            .prepare_highlighting(HighlightRequest::Diff(Arc::clone(&self.content)));
    }

    fn result(&self) -> HighlightingFinished {
        HighlightingFinished {
            request: HighlightRequest::Diff(Arc::clone(&self.content)),
            highlighted: self.syntax.highlight(
                &self.content.path,
                self.content.rows.clone(),
                self.content.old_content.as_deref(),
                self.content.new_content.as_deref(),
            ),
        }
    }
}

#[test]
fn delayed_colors_preserve_search_text_navigation_and_expanded_views() {
    let fixture = HighlightFixture::new("needle");
    for file_view in [false, true] {
        let mut loaded = LoadedDocument::new("a.rs");
        fixture.load(&mut loaded);
        let document = &mut loaded.document;
        if file_view {
            document.diff.show_file();
        } else {
            document.diff.expand_all();
        }
        let text = document.diff.search_document();
        let results = text_search::Request {
            id: 1,
            query: "needle".to_owned(),
            documents: vec![Arc::clone(&text)],
        }
        .search();
        assert_eq!(results.matches.len(), 1);
        document.cursor = results.matches[0].position.row;
        document.column = results.matches[0].position.column;
        document.scroll = 1;
        let location = (
            document.cursor,
            document.column,
            document.scroll,
            document.diff.len(),
        );
        let plain_rows = document.diff.rows.clone();
        document.finish_highlighting(&fixture.result());
        assert_eq!(
            (
                document.cursor,
                document.column,
                document.scroll,
                document.diff.len()
            ),
            location
        );
        assert_eq!(document.diff.is_file_view(), file_view);
        assert!(Arc::ptr_eq(&text, &document.diff.search_document()));
        assert_ne!(
            document.diff.rows, plain_rows,
            "syntax colors should be applied"
        );
        assert_eq!(
            document.diff.source_text(document.cursor).as_deref(),
            Some("fn needle() {}")
        );
        if file_view {
            document.diff.show_diff();
            assert!(document.diff.rows.iter().any(|row| {
                matches!(row, PresentedRow::Diff { tokens, .. } if tokens.iter().any(|token| token.color != Color::White))
            }));
        }
    }
}

#[test]
fn highlights_from_a_replaced_load_cannot_change_current_text() {
    let old = HighlightFixture::new("old_name");
    let current = HighlightFixture::new("current_name");
    let mut loaded = LoadedDocument::new("a.rs");
    old.load(&mut loaded);
    loaded.document.request_highlighting().unwrap();
    current.load(&mut loaded);
    let before = loaded.document.diff.clone();
    loaded.document.finish_highlighting(&old.result());
    assert_eq!(loaded.document.diff, before);
    loaded.document.finish_highlighting(&current.result());
    assert_ne!(loaded.document.diff.rows, before.rows);
    assert_eq!(
        loaded.document.diff.search_document(),
        before.search_document()
    );
}

#[test]
fn loading_an_unselected_diff_does_not_start_highlighting_or_lsp() {
    let (mut registry, _, target) = history_registry();
    select_files(&mut registry, ["b.rs"]);
    let fixture = HighlightFixture::new("needle");
    let actions = registry
        .publish((*fixture.content).clone())
        .unwrap()
        .into_iter()
        .flat_map(DispatchResult::into_actions)
        .collect::<Vec<_>>();
    assert!(
        !actions
            .iter()
            .any(|action| matches!(action, Action::Highlight(_) | Action::OpenLspDocument(_)))
    );
    let component = registry.get::<DiffComponent>(target).unwrap();
    let loaded = component
        .documents
        .iter()
        .find(|loaded| loaded.path == "a.rs")
        .unwrap();
    assert_eq!(
        text_search::Request {
            id: 1,
            query: "needle".to_owned(),
            documents: vec![loaded.document.diff.search_document()],
        }
        .search()
        .matches
        .len(),
        1
    );
    let selected = registry
        .publish(FileSelected {
            path: "a.rs".to_owned(),
        })
        .unwrap()
        .into_iter()
        .flat_map(DispatchResult::into_actions)
        .collect::<Vec<_>>();
    assert!(
        selected
            .iter()
            .any(|action| matches!(action, Action::Highlight(_)))
    );
    assert!(selected.contains(&Action::OpenLspDocument("a.rs".into())));
}
