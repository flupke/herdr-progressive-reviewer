use super::{RowKind, RowView, TokenView};

fn row(tokens: &[&str]) -> RowView {
    RowView {
        kind: RowKind::Context,
        old_line: Some(1),
        new_line: Some(1),
        tokens: tokens
            .iter()
            .map(|text| TokenView {
                text: (*text).to_owned(),
                role: None,
            })
            .collect(),
    }
}

fn texts(rows: &[RowView]) -> Vec<String> {
    rows.iter()
        .map(|row| row.tokens.iter().map(|token| token.text.as_str()).collect())
        .collect()
}

#[test]
fn cited_rows_lose_the_indentation_they_all_share() {
    let rows = RowView::dedented(vec![
        row(&["    ", "pub fn", " close()"]),
        row(&["        queue", ".flush();"]),
        row(&["    }"]),
    ]);
    assert_eq!(texts(&rows), ["pub fn close()", "    queue.flush();", "}"]);
}

#[test]
fn a_blank_row_does_not_hold_the_indentation() {
    let rows = RowView::dedented(vec![
        row(&["        a"]),
        row(&[""]),
        row(&["    "]),
        row(&["      b"]),
    ]);
    assert_eq!(texts(&rows), ["  a", "", "", "b"]);
}

#[test]
fn rows_that_start_at_the_margin_stay_as_they_are() {
    let rows = RowView::dedented(vec![row(&["fn main() {"]), row(&["    body"])]);
    assert_eq!(texts(&rows), ["fn main() {", "    body"]);
}
