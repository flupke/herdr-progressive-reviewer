use super::*;

struct ContextExpansion {
    registry: ComponentEventBus<Action>,
    target: ComponentTarget,
}

impl ContextExpansion {
    fn new() -> Self {
        let (mut registry, reviewable_files, target) = registry_with_observer();
        reviewable_files.replace(["src/lib.rs".to_owned()].into());
        publish_repository(&mut registry, "checkpoint");
        registry
            .publish(FileSelected {
                path: "src/lib.rs".to_owned(),
            })
            .unwrap();
        registry
            .publish(DiffViewportChanged {
                width: 78,
                height: 10,
            })
            .unwrap();
        let content = (1..=100)
            .map(|line| {
                if line == 24 {
                    "wrapped context ".repeat(60)
                } else {
                    format!("line {line}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let rows = [2, 25, 60]
            .into_iter()
            .flat_map(|start| {
                std::iter::once(DiffRow::Hunk {
                    old_start: start,
                    old_count: 4,
                    new_start: start,
                    new_count: 4,
                })
                .chain((start..start + 4).map(|line| DiffRow::Context {
                    old_line: line,
                    new_line: line,
                    text: format!(" line {line}"),
                }))
            })
            .collect();
        registry
            .publish(DiffContentLoaded {
                review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
                path: "src/lib.rs".to_owned(),
                rows,
                old_content: Some(content.as_bytes().to_vec()),
                new_content: Some(content.into_bytes()),
            })
            .unwrap();
        registry
            .publish(GuideLayoutChanged {
                items: vec![GuideItem {
                    target: GuideTarget::Lines {
                        path: "src/lib.rs".to_owned(),
                        old: None,
                        new: Some(GuideLineRange {
                            first_line: 25,
                            last_line: 28,
                        }),
                    },
                    text: "Review the current hunk.".to_owned(),
                    status: GuideItemStatus::Matched,
                }],
                counters: vec![Some(ui_events::GuideCounter {
                    number: 1,
                    total: 1,
                })],
            })
            .unwrap();
        Self { registry, target }
    }

    fn document(&self) -> &document::DiffDocument {
        &self
            .registry
            .get::<DiffComponent>(self.target)
            .unwrap()
            .displayed_document()
            .unwrap()
            .document
    }

    fn select_line(&mut self, line: u32) {
        let row = (0..self.document().diff.len())
            .find(|row| {
                self.document()
                    .diff
                    .source_position(*row)
                    .is_some_and(|(source, _)| source + 1 == line)
            })
            .unwrap();
        dispatch_key(&mut self.registry, self.target, Key::First);
        for _ in 0..row {
            dispatch_key(&mut self.registry, self.target, Key::Down);
        }
        for _ in 0..3 {
            dispatch_key(&mut self.registry, self.target, Key::Char('l'));
        }
    }

    fn screen(&self) -> Vec<String> {
        rendered_diff_with_guides_lines(&self.registry, self.target)
    }

    fn pointer(&mut self, kind: PointerInputKind, row: u16, column: u16) {
        self.registry
            .dispatch_hovered_input(
                &EventEnvelope::new(pointer_input(kind, row, column)),
                self.target,
            )
            .unwrap();
    }

    fn control(&mut self, control: DiffControl) {
        let component = self.registry.get::<DiffComponent>(self.target).unwrap();
        let column = (0..80)
            .find(|column| component.control_at(80, *column) == Some(control))
            .unwrap();
        self.pointer(PointerInputKind::Click, 0, column);
    }
}

struct HunkAnchor {
    location: ui_events::PresentationLocation,
    column: usize,
    screen_rows: Vec<(usize, String)>,
}

impl HunkAnchor {
    fn new(fixture: &ContextExpansion, line: u32) -> Self {
        let document = fixture.document();
        let screen_rows: Vec<_> = fixture
            .screen()
            .into_iter()
            .enumerate()
            .filter(|(_, text)| {
                text.contains(&format!("line {line}"))
                    || (line == 25 && text.contains("Review the current hunk."))
            })
            .collect();
        assert!(!screen_rows.is_empty());
        Self {
            location: document
                .diff
                .presentation_location(document.cursor)
                .unwrap(),
            column: document.column,
            screen_rows,
        }
    }

    fn assert_preserved(&self, fixture: &ContextExpansion) {
        let document = fixture.document();
        assert_eq!(
            document.diff.presentation_location(document.cursor),
            Some(self.location)
        );
        assert_eq!(document.column, self.column);
        let screen = fixture.screen();
        for (row, text) in &self.screen_rows {
            assert_eq!(&screen[*row], text);
        }
    }
}

#[test]
fn clicking_context_anchors_the_cursor_hunk_above_or_below_it() {
    for line in [5, 25] {
        let mut fixture = ContextExpansion::new();
        fixture.select_line(line);
        fixture.pointer(PointerInputKind::Scroll(1), 4, 4);
        let before = fixture.screen();
        let gap = before
            .iter()
            .position(|text| text.contains("19 unmodified lines"))
            .unwrap();
        let anchor = HunkAnchor::new(&fixture, line);
        fixture.pointer(PointerInputKind::Click, u16::try_from(gap).unwrap(), 4);
        anchor.assert_preserved(&fixture);
        assert!(
            !fixture
                .document()
                .diff
                .rows
                .iter()
                .any(|row| matches!(row, PresentedRow::Gap { start: 6, .. }))
        );
    }
}

#[test]
fn expand_and_contract_all_preserve_the_cursor_hunk_and_column() {
    for line in [5, 25, 60] {
        let mut fixture = ContextExpansion::new();
        fixture.select_line(line);
        let anchor = HunkAnchor::new(&fixture, line);
        fixture.control(DiffControl::ExpandAll);
        anchor.assert_preserved(&fixture);
        assert!(
            !fixture
                .document()
                .diff
                .rows
                .iter()
                .any(|row| matches!(row, PresentedRow::Gap { .. }))
        );
        fixture.control(DiffControl::ContractAll);
        anchor.assert_preserved(&fixture);
        assert!(
            !fixture
                .document()
                .diff
                .rows
                .iter()
                .any(|row| matches!(row, PresentedRow::Expanded { .. }))
        );
    }
}

#[test]
fn keyboard_expansion_keeps_the_context_boundary_in_place() {
    for key in [Key::Expand, Key::Char('l')] {
        let mut fixture = ContextExpansion::new();
        fixture.select_line(5);
        dispatch_key(&mut fixture.registry, fixture.target, Key::Down);
        let before = fixture.screen();
        let gap = before
            .iter()
            .position(|text| text.contains("19 unmodified lines"))
            .unwrap();
        dispatch_key(&mut fixture.registry, fixture.target, key);
        assert!(fixture.screen()[gap].contains("line 6"));
    }
}

#[test]
fn contracting_context_under_the_cursor_does_not_reopen_it() {
    let mut fixture = ContextExpansion::new();
    fixture.control(DiffControl::ExpandAll);
    fixture.select_line(12);
    fixture.control(DiffControl::ContractAll);
    let document = fixture.document();
    assert!(matches!(
        document.diff.rows[document.cursor],
        PresentedRow::Gap { start: 6, .. }
    ));
    assert!(
        !document
            .diff
            .rows
            .iter()
            .any(|row| matches!(row, PresentedRow::Expanded { .. }))
    );
}
