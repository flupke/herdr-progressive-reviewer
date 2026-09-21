use super::*;

struct Scrolling {
    registry: ComponentEventBus<Action>,
    target: ComponentTarget,
}

impl Scrolling {
    fn evidence_height(&self, width: u16, range: &GuideLineRange) -> u16 {
        let viewer = self.registry.get::<DiffComponent>(self.target).unwrap();
        let file = viewer.displayed_document().unwrap();
        let native = viewer
            .renderer(viewer.palette, None, false)
            .evidence_height(file, width, review_explore::SourceSide::New, Some(range));
        for _ in 0..2 {
            assert_eq!(
                viewer.measure_evidence_height(
                    file,
                    width,
                    review_explore::SourceSide::New,
                    Some(range),
                ),
                native,
            );
        }
        native
    }

    fn new(rows: Vec<DiffRow>, width: u16, height: u16) -> Self {
        let (mut registry, reviewable_files, target) = registry_with_observer();
        reviewable_files.replace(["src/lib.rs".to_owned()].into());
        publish_repository(&mut registry, "checkpoint");
        registry
            .publish(FileSelected {
                path: "src/lib.rs".to_owned(),
            })
            .unwrap();
        registry
            .publish(DiffViewportChanged { width, height })
            .unwrap();
        registry
            .publish(DiffContentLoaded {
                review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
                path: "src/lib.rs".to_owned(),
                rows,
                old_content: None,
                new_content: None,
            })
            .unwrap();
        Self { registry, target }
    }

    fn key(&mut self, key: Key) -> Vec<Action> {
        dispatch_key(&mut self.registry, self.target, key)
            .into_iter()
            .flat_map(DispatchResult::into_actions)
            .collect()
    }

    fn scroll(&mut self, delta: isize) {
        self.registry
            .dispatch_hovered_input(
                &EventEnvelope::new(PointerInput {
                    kind: PointerInputKind::Scroll(delta),
                    position: None,
                }),
                self.target,
            )
            .unwrap();
    }

    fn position(&self) -> (usize, usize, usize) {
        let document = &self
            .registry
            .get::<DiffComponent>(self.target)
            .unwrap()
            .displayed_document()
            .unwrap()
            .document;
        (document.cursor, document.column, document.scroll)
    }

    fn guide(&mut self) {
        self.registry
            .publish(GuideLayoutChanged {
                items: vec![GuideItem {
                    target: GuideTarget::Lines {
                        path: "src/lib.rs".to_owned(),
                        old: None,
                        new: Some(GuideLineRange {
                            first_line: 3,
                            last_line: 3,
                        }),
                    },
                    text: "A review guide.".to_owned(),
                    status: GuideItemStatus::Matched,
                }],
                counters: vec![Some(ui_events::GuideCounter {
                    number: 1,
                    total: 1,
                })],
            })
            .unwrap();
    }
}

#[test]
fn cached_evidence_size_tracks_wrapping_ranges_reload_and_inline_editing() {
    let mut fixture = Scrolling::new(context_rows(30), 78, 20);
    let range = GuideLineRange {
        first_line: 3,
        last_line: 5,
    };
    let wide = fixture.evidence_height(78, &range);
    let narrow = fixture.evidence_height(8, &range);
    assert!(narrow > wide);
    let larger_range = GuideLineRange {
        first_line: 3,
        last_line: 15,
    };
    assert!(fixture.evidence_height(78, &larger_range) > wide);
    assert_eq!(fixture.evidence_height(78, &range), wide);

    let text = "long replacement line with more wrapping ".repeat(6);
    fixture
        .registry
        .publish(DiffContentLoaded {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            path: "src/lib.rs".into(),
            rows: (1..=30)
                .map(|line| DiffRow::Add {
                    new_line: line,
                    text: format!("+{text}"),
                })
                .collect(),
            old_content: None,
            new_content: Some(format!("{text}\n").repeat(30).into_bytes()),
        })
        .unwrap();
    let reloaded = fixture.evidence_height(78, &range);
    assert!(reloaded > wide);

    fixture
        .registry
        .publish(ui_events::ReviewThreadsLoaded {
            review_unit: "change".into(),
            result: Ok(review_threads::ReviewThreads::new("change".into())),
        })
        .unwrap();
    fixture.key(Key::Down);
    fixture.key(Key::Down);
    fixture.key(Key::Char('a'));
    fixture
        .registry
        .publish(ui_events::TextPasted(
            "An inline explanation\nwith additional lines\nand more context".into(),
        ))
        .unwrap();
    let viewer = fixture
        .registry
        .get::<DiffComponent>(fixture.target)
        .unwrap();
    assert!(
        viewer
            .comments
            .inline_editor_visible_in(viewer.displayed_document().unwrap())
    );
    assert!(fixture.evidence_height(78, &range) > reloaded);
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
    let [Action::Lsp { query, .. }] = actions.as_slice() else {
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
fn scrolling_chooses_diff_lines_past_guide_rows_at_both_edges() {
    let mut fixture = Scrolling::new(context_rows(10), 78, 5);
    fixture.guide();
    fixture.scroll(2);
    assert_eq!(fixture.position(), (2, 0, 2));
    fixture.key(Key::Last);
    fixture.scroll(-100);
    assert_eq!(fixture.position(), (1, 0, 0));
}

#[test]
fn a_new_guide_keeps_the_cursor_in_the_remaining_visible_diff() {
    let mut fixture = Scrolling::new(context_rows(10), 78, 5);
    for _ in 0..3 {
        fixture.key(Key::Down);
    }
    assert_eq!(fixture.position(), (3, 0, 0));
    fixture.guide();
    assert_eq!(fixture.position(), (1, 0, 0));
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
    fixture
        .registry
        .publish(DiffContentLoaded {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            path: "src/lib.rs".to_owned(),
            rows,
            old_content: None,
            new_content: None,
        })
        .unwrap();
    assert_eq!(fixture.position(), (5, 0, 10));
}

#[test]
fn scrolling_extends_a_live_selection_but_preserves_a_fixed_selection() {
    let mut fixture = Scrolling::new(context_rows(30), 78, 5);
    fixture.key(Key::Visual);
    fixture.scroll(5);
    let component = fixture
        .registry
        .get::<DiffComponent>(fixture.target)
        .unwrap();
    assert_eq!(component.selection.unwrap().range(), 0..=5);
    fixture.key(Key::Visual);
    fixture.scroll(5);
    let component = fixture
        .registry
        .get::<DiffComponent>(fixture.target)
        .unwrap();
    assert_eq!(component.selection.unwrap().range(), 0..=5);
    assert_eq!(fixture.position(), (10, 0, 10));
}
