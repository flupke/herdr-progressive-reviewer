use review_hunks::{HunkMark, HunkSpan};
use review_repository::diff::parse_file_diff;
use review_repository::repository::ChangedFile;

use super::*;

/// Thirty lines; the change rewrites line 3, reviewed already, and line 20,
/// still open.
fn text(edits: &[(usize, &str)]) -> Vec<u8> {
    let mut lines = (1..=30)
        .map(|line| format!("line {line}"))
        .collect::<Vec<_>>();
    for (line, replacement) in edits {
        (*replacement).clone_into(&mut lines[*line - 1]);
    }
    lines
        .join("\n")
        .into_bytes()
        .into_iter()
        .chain([b'\n'])
        .collect()
}

fn rows(before: &[u8], after: &[u8]) -> Vec<DiffRow> {
    parse_file_diff(
        &review_hunks::unified_diff("src/lib.rs", before, after),
        &ChangedFile::modified("src/lib.rs"),
    )
}

struct HunkFixture {
    registry: ComponentEventBus<Action>,
    reviewable_files: ReviewableFiles,
    target: ComponentTarget,
}

impl HunkFixture {
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
        let mut review = Self {
            registry,
            reviewable_files,
            target,
        };
        review.load();
        review
    }

    fn load(&mut self) {
        self.load_reviewed(&[(3, "three")]);
    }

    /// Load the diff with the given lines already reviewed.
    fn load_reviewed(&mut self, reviewed: &[(usize, &str)]) {
        let base = text(&[]);
        let reviewed = text(reviewed);
        let current = text(&[(3, "three"), (20, "twenty")]);
        let open = rows(&reviewed, &current);
        self.registry
            .publish(DiffContentLoaded {
                review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
                path: "src/lib.rs".to_owned(),
                hunks: review_hunks::HunkReview::new(&base, &reviewed, &current).hunks(&open),
                rows: open,
                old_content: Some(reviewed),
                new_content: Some(current),
            })
            .unwrap();
    }

    fn screen(&self) -> Vec<String> {
        rendered_diff_lines(&self.registry, self.target)
    }

    /// Click the control drawn as `symbol`, after rendering the screen.
    fn click(&mut self, symbol: &str) -> Vec<Action> {
        let screen = self.screen();
        let (row, line) = screen
            .iter()
            .enumerate()
            .find(|(_, line)| line.contains(symbol))
            .unwrap_or_else(|| panic!("no {symbol} on screen:\n{}", screen.join("\n")));
        let column = line
            .chars()
            .position(|character| character.to_string() == symbol)
            .unwrap();
        self.registry
            .dispatch_hovered_input(
                &EventEnvelope::new(pointer_input(
                    PointerInputKind::Click,
                    u16::try_from(row).unwrap(),
                    u16::try_from(column).unwrap(),
                )),
                self.target,
            )
            .unwrap()
            .into_results()
            .into_iter()
            .flat_map(DispatchResult::into_actions)
            .collect()
    }

    fn shortcut(&mut self) -> Vec<Action> {
        dispatch_key(&mut self.registry, self.target, Key::Char('r'));
        self.registry
            .dispatch_global_input(&EventEnvelope::new(Key::Char('h')))
            .unwrap()
            .into_results()
            .into_iter()
            .flat_map(DispatchResult::into_actions)
            .collect()
    }

    fn rows(&self) -> &[PresentedRow] {
        &self
            .registry
            .get::<DiffComponent>(self.target)
            .unwrap()
            .files
            .displayed_document()
            .unwrap()
            .document
            .diff
            .rows
    }

    fn put_cursor_on_folded_hunk(&mut self) {
        let row = self
            .rows()
            .iter()
            .position(|row| matches!(row, PresentedRow::ReviewedHunk { .. }))
            .unwrap();
        dispatch_key(&mut self.registry, self.target, Key::First);
        for _ in 0..row {
            dispatch_key(&mut self.registry, self.target, Key::Down);
        }
    }

    fn saved(&mut self, state: ReviewState) -> Vec<Action> {
        self.registry
            .publish(ReviewStateSaved {
                review_unit: "change".into(),
                path: "src/lib.rs".to_owned(),
                result: Ok(state),
            })
            .unwrap()
            .into_iter()
            .flat_map(DispatchResult::into_actions)
            .collect()
    }
}

fn set_hunk(mark: HunkMark) -> Action {
    Action::Repository(RepositoryAction::SetHunkReviewed {
        review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
        path: "src/lib.rs".to_owned(),
        mark,
    })
}

fn twenty() -> HunkSpan {
    HunkSpan {
        old: 19..20,
        new: 19..20,
    }
}

fn three() -> HunkSpan {
    HunkSpan {
        old: 2..3,
        new: 2..3,
    }
}

#[test]
fn a_reviewed_hunk_is_folded_and_open_hunks_offer_their_control() {
    let review = HunkFixture::new();

    let screen = review.screen().join("\n");

    assert!(screen.contains("✓ reviewed hunk: +1 -1"), "{screen}");
    assert!(screen.contains('☑'), "{screen}");
    assert!(screen.contains('☐'), "{screen}");
    assert!(screen.contains("1/2 hunks reviewed"), "{screen}");
    assert!(
        !screen.contains("line 3"),
        "the folded hunk hides its lines"
    );
}

#[test]
fn clicking_an_open_hunk_control_accepts_that_hunk() {
    let mut review = HunkFixture::new();

    assert_eq!(
        review.click("☐"),
        vec![set_hunk(HunkMark::Review(twenty()))]
    );
}

#[test]
fn clicking_a_reviewed_hunk_control_reopens_it() {
    let mut review = HunkFixture::new();

    assert_eq!(
        review.click("☑"),
        vec![set_hunk(HunkMark::Unreview(three()))]
    );
}

#[test]
fn the_hunk_shortcut_toggles_the_hunk_at_the_cursor() {
    let mut review = HunkFixture::new();
    review.put_cursor_on_folded_hunk();

    assert_eq!(
        review.shortcut(),
        vec![set_hunk(HunkMark::Unreview(three()))]
    );
}

#[test]
fn expanding_a_folded_hunk_shows_its_original_change() {
    let mut review = HunkFixture::new();
    review.put_cursor_on_folded_hunk();

    dispatch_key(&mut review.registry, review.target, Key::Char('l'));

    let screen = review.screen().join("\n");
    assert!(screen.contains("line 3"), "{screen}");
    assert!(screen.contains("three"), "{screen}");
    assert!(screen.contains('☑'), "the expanded hunk keeps its control");
}

#[test]
fn the_next_mark_waits_for_the_diff_the_last_one_reloads() {
    let mut review = HunkFixture::new();
    assert_eq!(review.click("☐").len(), 1);

    assert!(
        review.click("☑").is_empty(),
        "the first mark is still saving"
    );
    let reload = review.saved(ReviewState::partially_reviewed(
        DiffStatistics::default(),
        None,
    ));
    assert_eq!(
        reload,
        vec![Action::Document(DocumentAction::Load(DocumentLoad::Diff {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            path: "src/lib.rs".to_owned(),
        }))]
    );
    assert!(review.click("☑").is_empty(), "the diff is still reloading");
    review.load();

    assert_eq!(
        review.click("☑"),
        vec![set_hunk(HunkMark::Unreview(three()))]
    );
}

#[test]
fn a_failed_reload_lets_the_next_mark_through() {
    let mut review = HunkFixture::new();
    assert_eq!(review.click("☐").len(), 1);
    review.saved(ReviewState::partially_reviewed(
        DiffStatistics::default(),
        None,
    ));

    review
        .registry
        .publish(DiffContentLoadFailed {
            review_checkpoint: ReviewCheckpoint::new("change", "checkpoint"),
            path: "src/lib.rs".to_owned(),
        })
        .unwrap();

    review.put_cursor_on_folded_hunk();
    assert_eq!(
        review.shortcut(),
        vec![set_hunk(HunkMark::Unreview(three()))]
    );
}

#[test]
fn a_file_that_no_longer_needs_review_offers_no_hunk_control() {
    let mut review = HunkFixture::new();
    review.put_cursor_on_folded_hunk();

    review
        .reviewable_files
        .replace(std::collections::HashSet::new());
    review.registry.publish(ReviewableFilesChanged).unwrap();

    assert!(review.shortcut().is_empty());
    assert!(!review.screen().join("\n").contains('☐'));
}

#[test]
fn a_hunk_accepted_at_the_cursor_folds_after_the_reload() {
    let mut review = HunkFixture::new();
    review.load_reviewed(&[]);
    // The cursor sits on the hunk's added line, "three".
    let row = review
        .screen()
        .iter()
        .skip(1)
        .position(|line| line.contains("three"))
        .unwrap();
    dispatch_key(&mut review.registry, review.target, Key::First);
    for _ in 0..row {
        dispatch_key(&mut review.registry, review.target, Key::Down);
    }

    review.load_reviewed(&[(3, "three")]);

    assert!(
        review
            .rows()
            .iter()
            .any(|row| matches!(row, PresentedRow::ReviewedHunk { .. })),
        "{:?}",
        review.screen()
    );
    assert!(
        !review
            .rows()
            .iter()
            .any(|row| matches!(row, PresentedRow::ReviewedLine { .. }))
    );
}

fn title(review: &HunkFixture, width: u16) -> String {
    let area = Rect::new(0, 0, width, 12);
    let mut buffer = Buffer::empty(area);
    review
        .registry
        .get::<DiffComponent>(review.target)
        .unwrap()
        .render(area, &mut buffer, Theme::default().palette, true);
    buffer.content()[..usize::from(width)]
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect()
}

#[test]
fn a_narrow_title_drops_the_hunk_count_before_the_path() {
    let review = HunkFixture::new();

    assert!(title(&review, 120).contains("Diff · src/lib.rs · 1/2 hunks reviewed"));
    let narrow = title(&review, 60);
    assert!(narrow.contains("Diff · src/lib.rs"), "{narrow}");
    assert!(!narrow.contains("hunks reviewed"), "{narrow}");
}

#[test]
fn a_file_that_no_longer_needs_review_shows_no_hunk_count() {
    let review = HunkFixture::new();

    review
        .reviewable_files
        .replace(std::collections::HashSet::new());

    assert!(!title(&review, 120).contains("hunks reviewed"));
}

#[test]
fn a_folded_hunk_fills_its_row_up_to_the_control() {
    let review = HunkFixture::new();
    let area = Rect::new(0, 0, 80, 12);
    let mut buffer = Buffer::empty(area);
    let palette = Theme::default().palette;
    review
        .registry
        .get::<DiffComponent>(review.target)
        .unwrap()
        .render(area, &mut buffer, palette, true);
    let row = (0..area.height)
        .find(|row| {
            (0..area.width)
                .map(|column| buffer[(column, *row)].symbol())
                .collect::<String>()
                .contains("reviewed hunk")
        })
        .unwrap();

    // Every cell between the text and the right border shares the fold's
    // background, the control included.
    for column in 40..area.width - 1 {
        assert_eq!(
            buffer[(column, row)].bg,
            palette.selection,
            "column {column}"
        );
    }
}
