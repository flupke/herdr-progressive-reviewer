use super::*;

/// `rows` rows of a QR code `columns` wide, inside a box.
fn qr_rows(rows: usize, columns: usize) -> Vec<String> {
    (0..rows)
        .map(|row| {
            let blocks: String = (0..columns)
                .map(|column| {
                    if (row + column) % 3 == 0 {
                        '▄'
                    } else {
                        '▀'
                    }
                })
                .collect();
            format!("│ {blocks}          │")
        })
        .collect()
}

#[test]
fn rows_lose_the_spaces_and_box_borders_that_end_them_only() {
    let screen = [
        " kuxnvmps  base     ",
        "╭ 日本語 ─╮╭─────╮",
        "│ 🦀 a    ││ c   │   ",
        "╰─────────╯╰─────╯ ",
    ]
    .join("\n");
    assert_eq!(
        whole(&screen),
        "0:  kuxnvmps  base\n1: ╭ 日本語 ─╮╭─────╮\n2: │ 🦀 a    ││ c\n3: ╰─────────╯╰─────╯"
    );
}

#[test]
fn a_side_or_cursor_that_lines_up_with_no_border_is_text() {
    let screen = [
        "╭ Threads ───╮",
        "│/query▏     │",
        "│ a.rs │     │",
        "│ b.rs       │",
        "╰────────────╯",
    ]
    .join("\n");
    assert_eq!(
        whole(&screen),
        "0: ╭ Threads ───╮\n1: │/query▏\n2: │ a.rs │\n3: │ b.rs\n4: ╰────────────╯"
    );
}

#[test]
fn one_empty_row_stays_a_line_and_a_run_folds_into_one() {
    let screen = [
        "╭ Files ──╮╭ Diff ──╮",
        "│ a.rs    ││ 1 fn   │",
        "│         ││        │",
        "│ b.rs    ││ 2      │",
        "│         ││        │",
        "│         ││        │",
        "          ",
        "╰─────────╯╰────────╯",
        "? help",
    ]
    .join("\n");
    assert_eq!(
        whole(&screen),
        [
            "0: ╭ Files ──╮╭ Diff ──╮",
            "1: │ a.rs    ││ 1 fn",
            "2:",
            "3: │ b.rs    ││ 2",
            "[empty rows 4-6]",
            "7: ╰─────────╯╰────────╯",
            "8: ? help",
        ]
        .join("\n")
    );
}

#[test]
fn a_qr_code_folds_into_one_placeholder_line() {
    let mut rows = vec!["│ Start a round from a phone:".to_owned()];
    rows.extend(qr_rows(QR_MIN_ROWS, QR_MIN_COLUMNS));
    rows.push("│ s start".into());
    assert_eq!(
        whole(&rows.join("\n")),
        "0: │ Start a round from a phone:\n│ [QR code, rows 1-11]\n12: │ s start"
    );
}

#[test]
fn blocks_too_few_rows_or_beside_text_are_no_qr_code() {
    let unfolded = |rows: Vec<String>| {
        let text = rows.join("\n");
        let lines: Vec<String> = whole(&text).lines().map(str::to_owned).collect();
        assert_eq!(lines.len(), rows.len(), "{text}");
        assert!(lines.iter().all(|line| line.contains('▀')), "{text}");
    };
    unfolded(qr_rows(QR_MIN_ROWS - 1, 37));
    unfolded(qr_rows(QR_MIN_ROWS, QR_MIN_COLUMNS - 1));
    unfolded(
        qr_rows(QR_MIN_ROWS, 37)
            .into_iter()
            .map(|row| format!("{row} Files"))
            .collect(),
    );
}

#[test]
fn a_qr_code_ends_where_its_columns_change() {
    let mut rows = qr_rows(QR_MIN_ROWS, 37);
    rows.extend(qr_rows(QR_MIN_ROWS, 25));
    assert_eq!(
        whole(&rows.join("\n")),
        "│ [QR code, rows 0-10]\n│ [QR code, rows 11-21]"
    );
}

#[test]
fn changes_hold_the_changed_rows_with_their_numbers() {
    let previous = "╭ Files ─╮\n│ ○ a.rs │\n│ ○ b.rs │\n│        │\n╰────────╯";
    let after = "╭ Files ─╮\n│ ○ a.rs │\n│ ● b.rs │\n│        │\n╰────────╯";
    let changes = changes(previous, after);
    assert_eq!(changes.count, 1);
    assert_eq!(changes.text, "2: │ ● b.rs");
}

#[test]
fn rows_that_empty_show_as_their_fold() {
    let previous = "Files\na.rs\nb.rs\nc.rs\n? help";
    let after = "Files\n\n   \n    \n? help";
    let changes = changes(previous, after);
    assert_eq!(changes.count, 3);
    assert_eq!(changes.text, "[empty rows 1-3]");
}

#[test]
fn only_rows_whose_shown_text_changed_count() {
    let previous = "Files\n│ a.rs   │\n";
    let after = "Files   \n│ a.rs   │\n";
    let changes = changes(previous, after);
    assert_eq!(changes.count, 0);
    assert_eq!(changes.text, "");
}
