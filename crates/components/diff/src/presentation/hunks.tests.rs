use ratatui::style::Color;
use review_repository::diff::parse_file_diff;
use review_repository::repository::ChangedFile;
use syntax_highlighting::SyntaxHighlighter;
use two_face::theme::EmbeddedThemeName;

use super::*;

fn text(lines: &[&str]) -> Vec<u8> {
    lines
        .iter()
        .flat_map(|line| line.bytes().chain([b'\n']))
        .collect()
}

fn rows(before: &[u8], after: &[u8]) -> Vec<DiffRow> {
    parse_file_diff(
        &review_hunks::unified_diff("f.rs", before, after),
        &ChangedFile::modified("f.rs"),
    )
}

/// A presentation of `current` with every hunk of `base` to `current` that
/// `reviewed` already contains folded.
fn presentation(base: &[u8], reviewed: &[u8], current: &[u8]) -> DiffPresentation {
    let open = rows(reviewed, current);
    let hunks = review_hunks::HunkReview::new(base, reviewed, current).hunks(&open);
    let highlighted = SyntaxHighlighter::new(EmbeddedThemeName::CatppuccinMocha, Color::White)
        .plain(open, Some(reviewed), Some(current));
    DiffPresentation::new(highlighted).with_hunks(hunks)
}

fn shape(presentation: &DiffPresentation) -> Vec<String> {
    presentation
        .rows
        .iter()
        .map(|row| match row {
            PresentedRow::Diff { source, .. } => match presentation.source_row(*source) {
                DiffRow::Context { text, .. }
                | DiffRow::Delete { text, .. }
                | DiffRow::Add { text, .. } => text.clone(),
                _ => String::new(),
            },
            PresentedRow::Gap { start, lines } => format!("gap {start}+{}", lines.len()),
            PresentedRow::Expanded { line, .. } => format!("line {line}"),
            PresentedRow::ReviewedHunk { hunk } => format!("reviewed {hunk}"),
            PresentedRow::ReviewedLine { hunk, row, .. } => format!("reviewed {hunk}.{row}"),
        })
        .collect()
}

const LINES: [&str; 20] = [
    "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16", "17",
    "18", "19", "20",
];

/// The base, the base with line 10 reviewed, and the current file that also
/// rewrites line 19.
fn versions() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let mut reviewed = LINES;
    reviewed[9] = "ten";
    let mut current = reviewed;
    current[18] = "nineteen";
    (text(&LINES), text(&reviewed), text(&current))
}

/// The open hunk's rows, as the diff shows them.
const OPEN_HUNK: [&str; 6] = [" 16", " 17", " 18", "-19", "+nineteen", " 20"];

fn with_open_hunk(rows: &[&str]) -> Vec<String> {
    rows.iter()
        .chain(&OPEN_HUNK)
        .map(|row| (*row).to_owned())
        .collect()
}

#[test]
fn a_reviewed_hunk_splits_the_unchanged_lines_around_it() {
    let (base, reviewed, current) = versions();

    let presentation = presentation(&base, &reviewed, &current);

    assert_eq!(
        shape(&presentation),
        with_open_hunk(&["gap 1+9", "reviewed 0", "gap 11+5"])
    );
    assert_eq!(presentation.hunk_badge(1), Some(HunkBadge::Reviewed));
    assert_eq!(
        presentation.hunk_mark(1),
        Some(HunkMark::Unreview(review_hunks::HunkSpan {
            old: 9..10,
            new: 9..10,
        }))
    );
}

#[test]
fn a_reviewed_deletion_sits_between_two_unchanged_lines() {
    let without_ten = LINES
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != 9)
        .map(|(_, line)| *line)
        .collect::<Vec<_>>();
    let mut current = without_ten.clone();
    current[17] = "nineteen";

    let presentation = presentation(&text(&LINES), &text(&without_ten), &text(&current));

    assert_eq!(
        shape(&presentation)[..3],
        ["gap 1+9", "reviewed 0", "gap 10+5"]
    );
}

#[test]
fn contracting_folds_expanded_reviewed_hunks_again() {
    let (base, reviewed, current) = versions();
    let mut presentation = presentation(&base, &reviewed, &current);

    assert!(presentation.expand(1));
    assert_eq!(
        shape(&presentation)[..4],
        ["gap 1+9", "reviewed 0.0", "reviewed 0.1", "gap 11+5"]
    );
    assert_eq!(presentation.hunk_badge(1), Some(HunkBadge::Reviewed));
    assert_eq!(presentation.hunk_badge(2), None);

    assert!(presentation.contract_all());
    assert_eq!(
        shape(&presentation)[..3],
        ["gap 1+9", "reviewed 0", "gap 11+5"]
    );
}

#[test]
fn only_the_first_row_of_an_open_hunk_carries_its_control() {
    let base = text(&LINES);
    let mut changed = LINES;
    changed[9] = "ten";
    let current = text(&changed);

    let presentation = presentation(&base, &base, &current);

    let badges = (0..presentation.len())
        .filter_map(|row| presentation.hunk_badge(row).map(|badge| (row, badge)))
        .collect::<Vec<_>>();
    assert_eq!(
        badges,
        [(
            1,
            HunkBadge::Open {
                since_review: false
            }
        )]
    );
    assert!(
        presentation.hunk_mark(3).is_some(),
        "any hunk row toggles it"
    );
    assert_eq!(presentation.hunk_mark(0), None);
}

#[test]
fn a_reviewed_hunk_in_an_open_hunks_context_replaces_those_lines() {
    let mut reviewed = LINES;
    reviewed[16] = "seventeen";
    let mut current = reviewed;
    current[18] = "nineteen";

    let presentation = presentation(&text(&LINES), &text(&reviewed), &text(&current));

    assert_eq!(
        shape(&presentation),
        [
            "gap 1+15",
            " 16",
            "reviewed 0",
            " 18",
            "-19",
            "+nineteen",
            " 20"
        ]
    );
}

#[test]
fn a_line_inside_a_folded_hunk_resolves_to_the_fold_and_unfolds_on_reveal() {
    let (base, reviewed, current) = versions();
    let mut presentation = presentation(&base, &reviewed, &current);

    assert_eq!(
        presentation.row_at_location(ui_events::PresentationLocation::NewLine(9)),
        Some(1)
    );

    let row = presentation.reveal_diff_line(9).unwrap();
    assert_eq!(
        presentation.presentation_location(row),
        Some(ui_events::PresentationLocation::NewLine(9))
    );
    assert!(matches!(
        presentation.rows[row],
        PresentedRow::ReviewedLine { .. }
    ));
}

#[test]
fn a_reviewed_hunk_running_from_unchanged_lines_into_context_shows_each_line_once() {
    let mut reviewed = LINES;
    for (line, text) in [
        (13, "fourteen"),
        (14, "fifteen"),
        (15, "sixteen"),
        (16, "seventeen"),
    ] {
        reviewed[line] = text;
    }
    let mut current = reviewed;
    current[18] = "nineteen";

    let presentation = presentation(&text(&LINES), &text(&reviewed), &text(&current));

    assert_eq!(
        shape(&presentation),
        ["gap 1+13", "reviewed 0", " 18", "-19", "+nineteen", " 20"]
    );
}

#[test]
fn revealing_a_line_of_an_expanded_reviewed_hunk_finds_its_row() {
    let (base, reviewed, current) = versions();
    let mut presentation = presentation(&base, &reviewed, &current);
    let row = presentation.reveal_diff_line(9).unwrap();

    assert_eq!(presentation.reveal_diff_line(9), Some(row));
}
