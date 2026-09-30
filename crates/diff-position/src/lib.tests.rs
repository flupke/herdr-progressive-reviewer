use super::*;

/// Columns one visual row holds before a source line wraps.
const WIDTH: usize = 10;

/// A layout of document rows that wrap every [`WIDTH`] columns, with comment
/// rows inserted after some of them.
struct TestLayout {
    rows: Vec<Option<(usize, usize)>>,
}

impl TestLayout {
    /// `segments[row]` visual rows for each document row, and `comments`
    /// rows below the document rows listed there.
    fn new(segments: &[usize], comments: &[(usize, usize)]) -> Self {
        let mut rows = Vec::new();
        for (row, count) in segments.iter().enumerate() {
            rows.extend((0..*count).map(|segment| Some((row, segment))));
            let comment_rows = comments
                .iter()
                .filter(|(below, _)| *below == row)
                .map(|(_, count)| *count)
                .sum::<usize>();
            rows.extend(std::iter::repeat_n(None, comment_rows));
        }
        Self { rows }
    }

    /// Every document row takes one visual row.
    fn lines(count: usize) -> Self {
        Self::new(&vec![1; count], &[])
    }

    fn document_rows(&self) -> usize {
        self.rows
            .iter()
            .flatten()
            .map(|(row, _)| row + 1)
            .max()
            .unwrap_or(0)
    }
}

impl Layout for TestLayout {
    fn row_count(&self) -> usize {
        self.rows.len()
    }

    fn cursor_row(&self, position: &Position) -> usize {
        let segment = position.column() / WIDTH;
        self.rows
            .iter()
            .enumerate()
            .filter(|(_, row)| row.is_some_and(|(row, _)| row == position.cursor()))
            .take(segment + 1)
            .map(|(index, _)| index)
            .last()
            .unwrap_or(0)
    }

    fn first_row_of(&self, row: usize) -> Option<usize> {
        self.rows
            .iter()
            .position(|candidate| candidate.is_some_and(|(candidate, _)| candidate == row))
    }

    fn nearest_cursor(&self, position: &Position, rows: Range<usize>) -> Option<(usize, usize)> {
        let cursor = self.cursor_row(position);
        let index = rows
            .filter(|index| self.rows[*index].is_some())
            .min_by_key(|index| index.abs_diff(cursor))?;
        let (row, segment) = self.rows[index]?;
        Some((row, segment * WIDTH + position.column() % WIDTH))
    }
}

/// The cursor's visual row is on the screen of `height` rows.
fn assert_cursor_on_screen(position: &Position, layout: &TestLayout, height: usize) {
    let cursor = layout.cursor_row(position);
    let top = position.top(layout.row_count());
    assert!(
        (top..top + height).contains(&cursor),
        "cursor row {cursor} is off the screen {top}..{}",
        top + height
    );
}

fn moved_to(row: usize, layout: &TestLayout, height: usize) -> Position {
    let mut position = Position::default();
    position.move_to(row, layout.document_rows());
    position.keep_cursor_visible(layout, height);
    position
}

#[test]
fn moving_the_cursor_off_either_edge_scrolls_just_enough() {
    let layout = TestLayout::lines(50);
    let mut position = moved_to(20, &layout, 10);
    assert_eq!(position.scroll(), 11);
    assert_cursor_on_screen(&position, &layout, 10);

    position.move_to(5, layout.document_rows());
    position.keep_cursor_visible(&layout, 10);
    assert_eq!(position.scroll(), 5);
    assert_cursor_on_screen(&position, &layout, 10);

    position.move_to(9, layout.document_rows());
    position.keep_cursor_visible(&layout, 10);
    assert_eq!(position.scroll(), 5, "a cursor on screen does not scroll");
}

#[test]
fn the_cursor_stays_inside_the_document() {
    let layout = TestLayout::lines(5);
    let position = moved_to(100, &layout, 10);
    assert_eq!(position.cursor(), 4);
    assert_eq!(position.scroll(), 0);
}

#[test]
fn a_wrapped_cursor_is_kept_visible_on_its_own_visual_row() {
    let layout = TestLayout::new(&[1, 1, 1, 4, 1], &[]);
    let mut position = Position::default();
    position.move_to(3, layout.document_rows());
    position.set_column(35);
    position.keep_cursor_visible(&layout, 2);
    assert_eq!(layout.cursor_row(&position), 6);
    assert_eq!(position.scroll(), 5);
    assert_cursor_on_screen(&position, &layout, 2);
}

#[test]
fn scrolling_brings_the_cursor_along_to_the_nearest_source_row() {
    let layout = TestLayout::new(&[1; 30], &[(12, 3)]);
    let mut position = moved_to(0, &layout, 10);
    position.set_column(4);

    assert!(position.scroll_by(11, &layout, 10));
    assert_eq!(position.scroll(), 11);
    assert_eq!(
        position.cursor(),
        11,
        "the nearest row on screen is the top one"
    );
    assert_eq!(position.column(), 4);
    assert_cursor_on_screen(&position, &layout, 10);

    assert!(position.scroll_by(1, &layout, 10));
    assert_eq!(position.cursor(), 12);

    assert!(position.scroll_by(1, &layout, 10));
    assert_eq!(position.scroll(), 13);
    assert_eq!(
        position.cursor(),
        13,
        "comment rows at the top are skipped for the next source row"
    );
    assert_cursor_on_screen(&position, &layout, 10);

    assert!(
        !position.scroll_by(-1, &layout, 10),
        "the cursor is still on screen"
    );
    assert_eq!(position.cursor(), 13);

    assert!(position.scroll_by(-100, &layout, 10));
    assert_eq!(position.scroll(), 0);
    assert_eq!(
        position.cursor(),
        9,
        "the nearest row on screen is the bottom one"
    );
    assert_cursor_on_screen(&position, &layout, 10);
}

#[test]
fn scrolling_stops_at_the_last_full_screen() {
    let layout = TestLayout::lines(30);
    let mut position = Position::default();
    assert!(position.scroll_by(100, &layout, 10));
    assert_eq!(position.scroll(), 20);
    assert_cursor_on_screen(&position, &layout, 10);

    let short = TestLayout::lines(4);
    let mut position = Position::default();
    assert!(!position.scroll_by(3, &short, 10));
    assert_eq!(position.scroll(), 0);
}

#[test]
fn shrinking_the_screen_keeps_the_cursor_visible() {
    let layout = TestLayout::lines(50);
    let mut position = moved_to(29, &layout, 20);
    assert_eq!(position.scroll(), 10);

    position.keep_cursor_visible(&layout, 5);
    assert_eq!(position.scroll(), 25);
    assert_cursor_on_screen(&position, &layout, 5);
}

#[test]
fn growing_the_screen_fills_it_from_the_end_of_the_document() {
    let layout = TestLayout::lines(30);
    let mut position = moved_to(29, &layout, 5);
    assert_eq!(position.scroll(), 25);

    position.keep_cursor_visible(&layout, 20);
    assert_eq!(position.scroll(), 10);
    assert_cursor_on_screen(&position, &layout, 20);
}

#[test]
fn folding_context_above_the_cursor_keeps_it_on_its_screen_row() {
    let unfolded = TestLayout::lines(40);
    let mut position = moved_to(30, &unfolded, 10);
    let screen_row = unfolded.cursor_row(&position) - position.scroll();
    let anchor = position.screen_anchor(&unfolded);

    // Rows 10 to 19 fold into one gap row: the cursor's document row moves up.
    let folded = TestLayout::lines(31);
    position.move_to(21, folded.document_rows());
    position.return_to(anchor, &folded);

    assert_eq!(folded.cursor_row(&position) - position.scroll(), screen_row);
    assert_cursor_on_screen(&position, &folded, 10);
}

#[test]
fn unfolding_context_above_the_cursor_keeps_it_on_its_screen_row() {
    let folded = TestLayout::lines(31);
    let mut position = moved_to(21, &folded, 10);
    let screen_row = folded.cursor_row(&position) - position.scroll();
    let anchor = position.screen_anchor(&folded);

    let unfolded = TestLayout::lines(40);
    position.move_to(30, unfolded.document_rows());
    position.return_to(anchor, &unfolded);

    assert_eq!(
        unfolded.cursor_row(&position) - position.scroll(),
        screen_row
    );
    assert_cursor_on_screen(&position, &unfolded, 10);
}

#[test]
fn jumps_center_the_cursor_or_align_its_row_to_the_top() {
    let layout = TestLayout::new(&[1, 1, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1], &[]);
    let mut position = Position::default();
    position.move_to(8, layout.document_rows());
    position.center_cursor(&layout, 6);
    assert_eq!(position.scroll(), 7);
    assert_cursor_on_screen(&position, &layout, 6);

    position.move_to(2, layout.document_rows());
    position.set_column(25);
    position.align_cursor_top(&layout);
    assert_eq!(
        position.scroll(),
        2,
        "the row's first visual row is at the top"
    );
    assert_cursor_on_screen(&position, &layout, 6);

    position.move_to(14, layout.document_rows());
    position.center_cursor(&layout, 6);
    assert_eq!(position.scroll(), 11, "the screen stays full at the end");
}

#[test]
fn an_edited_comment_is_revealed_whole_when_it_fits() {
    let layout = TestLayout::new(&[1; 30], &[(20, 4)]);
    let mut position = moved_to(20, &layout, 10);
    assert_eq!(position.scroll(), 11);

    position.reveal_comment(21..=24, true, &layout, 10);
    assert_eq!(position.scroll(), 15, "the editor's end comes on screen");

    position.reveal_comment(21..=24, true, &layout, 3);
    assert_eq!(position.scroll(), 22, "a tall editor shows its end");
}

#[test]
fn a_read_comment_is_revealed_from_the_row_above_it() {
    let layout = TestLayout::new(&[1; 30], &[(20, 4)]);
    let mut position = Position::default();
    position.reveal_comment(21..=24, false, &layout, 10);
    assert_eq!(position.scroll(), 20);

    position.reveal_comment(21..=24, false, &layout, 10);
    assert_eq!(position.scroll(), 20, "a comment on screen does not scroll");
}

#[test]
fn a_pinned_row_keeps_its_screen_row() {
    let layout = TestLayout::lines(30);
    let mut position = Position::default();
    position.pin(15, 3, &layout, 10);
    assert_eq!(position.scroll(), 12);

    position.pin(28, 0, &layout, 10);
    assert_eq!(position.scroll(), 20, "the screen stays full at the end");
}

#[test]
fn evidence_is_revealed_with_context_above_it() {
    let mut position = Position::default();
    position.reveal_evidence(10..12, 40, 10);
    assert_eq!(position.scroll(), 7);

    position.reveal_evidence(10..30, 40, 10);
    assert_eq!(position.scroll(), 10, "long evidence starts at the top");

    position.reveal_evidence(38..39, 40, 10);
    assert_eq!(position.scroll(), 30, "the screen stays full at the end");
}

#[test]
fn a_saved_position_is_fitted_to_the_document() {
    let mut position = Position::default();
    position.restore(Position::new(3, 7, 2), 10);
    assert_eq!(position, Position::new(3, 7, 2));

    position.restore(Position::new(30, 7, 20), 10);
    assert_eq!((position.cursor(), position.scroll()), (9, 9));

    position.restore_filling_screen(Position::new(30, 7, 20), 10, 4);
    assert_eq!((position.cursor(), position.scroll()), (9, 6));
}

#[test]
fn an_edited_comment_stays_in_view_when_the_screen_shrinks() {
    let layout = TestLayout::new(&[1; 30], &[(20, 4)]);
    let mut position = moved_to(20, &layout, 20);
    position.reveal_comment(21..=24, true, &layout, 20);
    assert_eq!(position.scroll(), 5);

    position.reveal_comment(21..=24, true, &layout, 6);
    let top = position.top(layout.row_count());
    assert!(
        top <= 21 && 24 < top + 6,
        "the editor rows 21..=24 are on the screen {top}..{}",
        top + 6
    );
}
