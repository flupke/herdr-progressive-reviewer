use super::*;
use ui_actions::LspAction;

/// A Files viewer driven directly, without the pane around it.
struct Scrolling {
    _bus: ComponentEventBus<Action>,
    viewer: SourceViewer,
    shortcuts: ui_shortcuts::ShortcutMatcher<DiffPaneCommand>,
}

/// Receives the viewer's publications while nothing else listens.
struct Detached;

impl Component<Action> for Detached {
    fn register_subscriptions(_: &mut ComponentSubscriptions<'_, Self, Action>) {}
}

impl Scrolling {
    fn new(rows: Vec<DiffRow>, width: u16, height: u16) -> Self {
        let mut bus = ComponentEventBus::new();
        let mut events = None;
        bus.mount(|publisher| {
            events = Some(publisher);
            Detached
        });
        let services = Services {
            events: events.unwrap(),
            highlighter: SyntaxHighlighter::new(EmbeddedThemeName::CatppuccinMocha, Color::White),
            repository_root: PathBuf::new(),
            palette: Theme::default().palette,
            drafts: std::rc::Rc::default(),
            thread_projection: review_thread_projection::SharedThreadProjection::default(),
        };
        let reviewable_files = ReviewableFiles::default();
        reviewable_files.replace(["src/lib.rs".to_owned()].into());
        let mut viewer = SourceViewer::new(&services, Role::Files, reviewable_files);
        viewer.repository_changed(&repository_event("checkpoint"));
        viewer.file_selected(&FileSelected {
            path: "src/lib.rs".to_owned(),
        });
        viewer.viewport_changed(&DiffViewportChanged { width, height });
        let mut fixture = Self {
            _bus: bus,
            viewer,
            shortcuts: ui_shortcuts::ShortcutMatcher::new(),
        };
        fixture.load(rows);
        fixture
    }

    fn load(&mut self, rows: Vec<DiffRow>) {
        self.viewer.content_loaded(&DiffContentLoaded {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            path: "src/lib.rs".to_owned(),
            rows,
            old_content: None,
            new_content: None,
        });
    }

    fn key(&mut self, key: Key) -> Vec<Action> {
        match self.viewer.resolve_key(key, &mut self.shortcuts) {
            component_core::InputResolution::Matched(input) => self.viewer.keyboard_input(input),
            _ => Vec::new(),
        }
    }

    fn scroll(&mut self, delta: isize) {
        self.viewer.pointer_input(PointerInput {
            kind: PointerInputKind::Scroll(delta),
            position: None,
        });
    }

    fn position(&self) -> (usize, usize, usize) {
        let position = self
            .viewer
            .displayed_document()
            .unwrap()
            .document
            .position();
        (position.cursor(), position.column(), position.scroll())
    }
}

#[test]
fn scrolling_moves_an_outside_cursor_to_the_nearest_visible_line() {
    let mut fixture = Scrolling::new(context_rows(30), 78, 5);
    fixture.key(Key::Down);
    fixture.key(Key::Down);
    for _ in 0..3 {
        fixture.key(Key::Char('l'));
    }
    fixture.scroll(1);
    assert_eq!(fixture.position(), (2, 3, 1));
    fixture.scroll(2);
    assert_eq!(fixture.position(), (3, 3, 3));
    fixture.scroll(100);
    assert_eq!(fixture.position(), (25, 3, 25));
    fixture.scroll(-1);
    assert_eq!(fixture.position(), (25, 3, 24));
    fixture.scroll(-100);
    assert_eq!(fixture.position(), (4, 3, 0));
    fixture.scroll(-1);
    assert_eq!(fixture.position(), (4, 3, 0));
    let actions = fixture.key(Key::Char('K'));
    let [Action::Lsp(LspAction::Request { query, .. })] = actions.as_slice() else {
        panic!("expected the visible cursor's LSP location");
    };
    assert_eq!((query.line, query.byte_column), (4, 3));
}

#[test]
fn scrolling_moves_the_cursor_between_wrapped_segments() {
    let mut fixture = Scrolling::new(
        vec![DiffRow::Context {
            old_line: 1,
            new_line: 1,
            text: format!(" {}", "a".repeat(80)),
        }],
        14,
        3,
    );
    fixture.scroll(1);
    assert_eq!(fixture.position(), (0, 10, 1));
    fixture.scroll(100);
    assert_eq!(fixture.position(), (0, 50, 5));
    fixture.scroll(-100);
    assert_eq!(fixture.position(), (0, 20, 0));
}

#[test]
fn scrolling_rechecks_visibility_after_an_end_of_line_cursor_moves() {
    let mut rows = vec![DiffRow::Context {
        old_line: 1,
        new_line: 1,
        text: format!(" {}", "a".repeat(18)),
    }];
    rows.extend((2..=11).map(|line| DiffRow::Context {
        old_line: line,
        new_line: line,
        text: format!(" line {line}"),
    }));
    let mut fixture = Scrolling::new(rows, 14, 3);
    fixture.key(Key::Char('$'));
    fixture.scroll(3);
    assert_eq!(fixture.position(), (2, 0, 3));
}

#[test]
fn scrolling_wrapped_unicode_keeps_grapheme_and_byte_positions_aligned() {
    let family = "👨‍👩‍👧‍👦";
    let mut fixture = Scrolling::new(
        vec![DiffRow::Context {
            old_line: 1,
            new_line: 1,
            text: format!(" {}", format!("abcdefghij{family}\t界").repeat(6)),
        }],
        14,
        3,
    );
    fixture.scroll(1);
    assert_eq!(fixture.position(), (0, 10, 1));
    fixture.scroll(1);
    assert_eq!(fixture.position(), (0, 10 + family.len() + 1, 2));
}

#[test]
fn reloaded_wrapped_content_keeps_the_cursor_inside_the_viewport() {
    let mut fixture = Scrolling::new(context_rows(30), 14, 3);
    fixture.scroll(10);
    let mut rows = context_rows(30);
    rows[0] = DiffRow::Context {
        old_line: 1,
        new_line: 1,
        text: format!(" {}", "a".repeat(72)),
    };
    fixture.load(rows);
    assert_eq!(fixture.position(), (5, 0, 10));
}

#[test]
fn scrolling_extends_a_live_selection_but_preserves_a_fixed_selection() {
    let mut fixture = Scrolling::new(context_rows(30), 78, 5);
    fixture.key(Key::Char('V'));
    fixture.scroll(5);
    assert_eq!(fixture.viewer.selected_rows(), Some(0..=5));
    fixture.key(Key::Char('V'));
    fixture.scroll(5);
    assert_eq!(fixture.viewer.selected_rows(), Some(0..=5));
    assert_eq!(fixture.position(), (10, 0, 10));
}
